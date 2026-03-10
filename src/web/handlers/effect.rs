// Effect/plugin management endpoints, ported from webserver.py
// /effect/list, /effect/get, /effect/add, /effect/remove, /effect/connect, /effect/disconnect,
// /effect/parameter/*, /effect/preset/*, /effect/image/*, /effect/install

use actix_web::{get, post, web, HttpResponse};
use serde::Deserialize;
use serde_json::json;

use crate::lv2_utils;
use crate::AppState;

/// GET /effect/list - list all available plugins (served from cache)
#[get("/effect/list")]
pub async fn effect_list(state: web::Data<AppState>) -> HttpResponse {
    let plugins = state.plugin_cache.get_plugins().await;
    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(plugins)
}

#[derive(Deserialize)]
pub struct UriQuery {
    uri: Option<String>,
}

/// GET /effect/get - get plugin info (cached)
#[get("/effect/get")]
pub async fn effect_get(query: web::Query<UriQuery>) -> HttpResponse {
    let uri = query.uri.as_deref().unwrap_or("");
    let info = lv2_utils::get_plugin_info(uri).unwrap_or(json!({}));
    HttpResponse::Ok()
        .insert_header(("Cache-Control", "public, max-age=31536000"))
        .json(info)
}

/// GET /effect/get_non_cached - get plugin info (not cached)
#[get("/effect/get_non_cached")]
pub async fn effect_get_non_cached(query: web::Query<UriQuery>) -> HttpResponse {
    let uri = query.uri.as_deref().unwrap_or("");
    let info = lv2_utils::get_non_cached_plugin_info(uri).unwrap_or(json!({}));
    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(info)
}

/// POST /effect/bulk - get info for multiple plugins
#[post("/effect/bulk")]
pub async fn effect_bulk(body: web::Json<Vec<String>>) -> HttpResponse {
    let mut result = serde_json::Map::new();
    for uri in body.iter() {
        if let Some(info) = lv2_utils::get_plugin_info(uri) {
            result.insert(uri.clone(), info);
        }
    }
    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(result)
}

#[derive(Deserialize)]
pub struct AddQuery {
    uri: String,
    x: Option<f64>,
    y: Option<f64>,
    #[serde(rename = "_")]
    _cachebust: Option<String>,
}

/// GET /effect/add/{instance:.*} - add a plugin (instance is like /graph/_2Voices)
#[get("/effect/add/{instance:.*}")]
pub async fn effect_add(
    path: web::Path<String>,
    query: web::Query<AddQuery>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let instance = format!("/{}", path.into_inner().trim_start_matches('/'));
    let x = query.x.unwrap_or(0.0) as i32;
    let y = query.y.unwrap_or(0.0) as i32;

    let mut session = state.session.write().await;
    session.web_add(&instance, &query.uri, x, y).await;

    // Return the full plugin info (the JS uses it to render the plugin on the pedalboard)
    match lv2_utils::get_plugin_info(&query.uri) {
        Some(data) => HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(data),
        None => HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(false),
    }
}

/// GET /effect/remove/{instance:.*} - remove a plugin
#[get("/effect/remove/{instance:.*}")]
pub async fn effect_remove(
    path: web::Path<String>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let instance = format!("/{}", path.into_inner().trim_start_matches('/'));
    let mut session = state.session.write().await;
    session.web_remove(&instance).await;

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(true)
}

/// GET /effect/connect/{ports:.*} - connect two ports (paths like /graph/capture_1,/graph/_2Voices/In)
#[get("/effect/connect/{ports:.*}")]
pub async fn effect_connect(
    path: web::Path<String>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let ports = path.into_inner();
    let ports = format!("/{}", ports.trim_start_matches('/'));
    if let Some((from, to)) = ports.split_once(',') {
        let mut session = state.session.write().await;
        session.web_connect(from, to).await;
        HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(true)
    } else {
        HttpResponse::BadRequest().json(false)
    }
}

/// GET /effect/disconnect/{ports:.*} - disconnect two ports
#[get("/effect/disconnect/{ports:.*}")]
pub async fn effect_disconnect(
    path: web::Path<String>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let ports = path.into_inner();
    let ports = format!("/{}", ports.trim_start_matches('/'));
    if let Some((from, to)) = ports.split_once(',') {
        let mut session = state.session.write().await;
        session.web_disconnect(from, to).await;
        HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(true)
    } else {
        HttpResponse::BadRequest().json(false)
    }
}

/// POST /effect/parameter/address/{port:.*} - address a parameter to a control
#[post("/effect/parameter/address/{port:.*}")]
pub async fn effect_parameter_address(
    path: web::Path<String>,
    body: String,
    state: web::Data<AppState>,
) -> HttpResponse {
    let port = format!("/{}", path.into_inner().trim_start_matches('/'));
    let addressing: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
    let mut session = state.session.write().await;
    session.web_parameter_address(&port, &addressing).await;

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(true)
}

/// POST /effect/parameter/set - set a parameter value via HMI
/// Body is a JSON string like "/graph/_2Voices/:bypass/0.0"
/// Format: symbol/instance/portsymbol/value (rsplit by "/" into 4 parts)
#[post("/effect/parameter/set{_:/?}")]
pub async fn effect_parameter_set(
    body: web::Json<serde_json::Value>,
    state: web::Data<AppState>,
) -> HttpResponse {
    if let Some(data) = body.as_str() {
        // Python: value, portsymbol, instance, symbol = data.rsplit("/", 3)
        // rsplitn reverses: parts[0]=value, parts[1]=portsymbol, parts[2]=rest of instance path
        let parts: Vec<&str> = data.rsplitn(4, '/').collect();
        if parts.len() == 4 {
            let value_str = parts[0];
            let portsymbol = parts[1];
            if let Ok(value) = value_str.parse::<f64>() {
                // Reconstruct instance — parts[3] is the leading segment, parts[2] is the rest
                // e.g. "/graph/_2Voices/:bypass/0.0" → parts = ["0.0", ":bypass", "graph/_2Voices", ""]
                let instance = format!("/{}", parts[2]);
                let port = format!("{}/{}", instance, portsymbol);
                let mut session = state.session.write().await;
                session.ws_parameter_set(&port, value, None).await;
            }
        }
    }
    // Return true even if we can't parse — avoid UI "Bug" notifications
    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(true)
}

/// GET /effect/preset/load/{instance} - load a preset
#[get("/effect/preset/load/{instance:.*}")]
pub async fn effect_preset_load(
    path: web::Path<String>,
    query: web::Query<UriQuery>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let raw = path.into_inner();
    let instance = if raw.starts_with('/') { raw } else { format!("/{}", raw) };
    let uri = match query.uri.as_deref() {
        Some(u) if !u.is_empty() => u,
        _ => {
            return HttpResponse::BadRequest()
                .insert_header(("Cache-Control", "no-store"))
                .json(false);
        }
    };

    let mut session = state.session.write().await;
    let ws_broadcast = session.ws_broadcast.clone();
    let msg_cb = |msg: &str| {
        if let Some(ref tx) = ws_broadcast {
            let _ = tx.send(msg.to_string());
        }
    };

    let ok = session.host.preset_load(&instance, uri, &msg_cb).await;

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(ok)
}

#[derive(Deserialize)]
pub struct PresetNameQuery {
    name: Option<String>,
}

/// GET /effect/preset/save_new/{instance} - save a new preset
#[get("/effect/preset/save_new/{instance:.*}")]
pub async fn effect_preset_save_new(
    path: web::Path<String>,
    query: web::Query<PresetNameQuery>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let raw = path.into_inner();
    let instance = if raw.starts_with('/') { raw } else { format!("/{}", raw) };
    let name = query.name.as_deref().unwrap_or("");
    if name.is_empty() {
        return HttpResponse::BadRequest()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({"ok": false}));
    }

    let mut session = state.session.write().await;
    match session
        .host
        .preset_save_new(&instance, name, &state.settings)
        .await
    {
        Some((bundle, uri)) => HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({"ok": true, "bundle": bundle, "uri": uri})),
        None => HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({"ok": false})),
    }
}

#[derive(Deserialize)]
pub struct PresetReplaceQuery {
    uri: Option<String>,
    bundle: Option<String>,
    name: Option<String>,
}

/// GET /effect/preset/save_replace/{instance} - replace an existing preset
#[get("/effect/preset/save_replace/{instance:.*}")]
pub async fn effect_preset_save_replace(
    path: web::Path<String>,
    query: web::Query<PresetReplaceQuery>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let raw = path.into_inner();
    let instance = if raw.starts_with('/') { raw } else { format!("/{}", raw) };
    let uri = query.uri.as_deref().unwrap_or("");
    let bundle = query.bundle.as_deref().unwrap_or("");
    let name = query.name.as_deref().unwrap_or("");

    if uri.is_empty() || bundle.is_empty() || name.is_empty() {
        return HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({"ok": false}));
    }

    let mut session = state.session.write().await;
    match session
        .host
        .preset_save_replace(&instance, uri, bundle, name)
        .await
    {
        Some((new_bundle, new_uri)) => HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({"ok": true, "bundle": new_bundle, "uri": new_uri})),
        None => HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({"ok": false})),
    }
}

/// GET /effect/preset/delete/{instance} - delete a preset
#[get("/effect/preset/delete/{instance:.*}")]
pub async fn effect_preset_delete(
    path: web::Path<String>,
    query: web::Query<PresetReplaceQuery>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let raw = path.into_inner();
    let instance = if raw.starts_with('/') { raw } else { format!("/{}", raw) };
    let uri = query.uri.as_deref().unwrap_or("");
    let bundle = query.bundle.as_deref().unwrap_or("");

    if uri.is_empty() || bundle.is_empty() {
        return HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(false);
    }

    let mut session = state.session.write().await;
    let ws_broadcast = session.ws_broadcast.clone();
    let msg_cb = |msg: &str| {
        if let Some(ref tx) = ws_broadcast {
            let _ = tx.send(msg.to_string());
        }
    };

    let ok = session
        .host
        .preset_delete(&instance, uri, bundle, &msg_cb)
        .await;

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(ok)
}

/// GET /effect/image/{type}.png - get plugin screenshot/thumbnail
#[get("/effect/image/{imgtype}.png")]
pub async fn effect_image(
    path: web::Path<String>,
    query: web::Query<UriQuery>,
) -> HttpResponse {
    let imgtype = path.into_inner(); // "screenshot" or "thumbnail"
    let uri = match query.uri.as_deref() {
        Some(u) => u,
        None => return HttpResponse::BadRequest().finish(),
    };

    let gui = match lv2_utils::get_plugin_gui_mini(uri) {
        Some(g) => g,
        None => return HttpResponse::NotFound().finish(),
    };

    let image_path = gui.get(&imgtype).and_then(|v| v.as_str()).unwrap_or("");

    if image_path.is_empty() || !std::path::Path::new(image_path).exists() {
        return HttpResponse::NotFound().finish();
    }

    match std::fs::read(image_path) {
        Ok(data) => HttpResponse::Ok()
            .content_type("image/png")
            .insert_header(("Cache-Control", "public, max-age=31536000"))
            .body(data),
        Err(_) => HttpResponse::NotFound().finish(),
    }
}

/// GET /effect/file/{prop} - serve a plugin GUI file (iconTemplate, settingsTemplate, stylesheet, javascript, etc.)
#[get("/effect/file/{prop}")]
pub async fn effect_file(
    path: web::Path<String>,
    query: web::Query<UriQuery>,
) -> HttpResponse {
    let prop = path.into_inner();
    let uri = match query.uri.as_deref() {
        Some(u) => u,
        None => return HttpResponse::BadRequest().finish(),
    };

    let gui = match lv2_utils::get_plugin_gui(uri) {
        Some(g) => g,
        None => return HttpResponse::NotFound().finish(),
    };

    let file_path = gui.get(&prop).and_then(|v| v.as_str()).unwrap_or("");

    if file_path.is_empty() || !std::path::Path::new(file_path).exists() {
        return HttpResponse::NotFound().finish();
    }

    let content_type = match prop.as_str() {
        "iconTemplate" | "settingsTemplate" | "stylesheet" | "javascript" => {
            "text/plain; charset=UTF-8"
        }
        _ => "application/octet-stream",
    };

    match std::fs::read(file_path) {
        Ok(data) => HttpResponse::Ok()
            .content_type(content_type)
            .insert_header(("Cache-Control", "public, max-age=31536000"))
            .body(data),
        Err(_) => HttpResponse::NotFound().finish(),
    }
}

/// GET /resources/{path:.*} - serve plugin GUI resources or shared resources
#[get("/resources/{path:.*}")]
pub async fn effect_resource(
    path: web::Path<String>,
    query: web::Query<UriQuery>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let resource_path = path.into_inner();

    // If uri is provided, try serving from the plugin's resourcesDirectory first
    if let Some(uri) = query.uri.as_deref() {
        if let Some(gui) = lv2_utils::get_plugin_gui_mini(uri) {
            let res_dir = gui.get("resourcesDirectory").and_then(|v| v.as_str()).unwrap_or("");
            if !res_dir.is_empty() {
                let full_path = std::path::Path::new(res_dir).join(&resource_path);
                if full_path.exists() {
                    if let Ok(data) = std::fs::read(&full_path) {
                        let content_type = mime_guess::from_path(&full_path)
                            .first_or_octet_stream()
                            .to_string();
                        return HttpResponse::Ok()
                            .content_type(content_type)
                            .insert_header(("Cache-Control", "public, max-age=31536000"))
                            .body(data);
                    }
                }
            }
        }
    }

    // Fallback to shared resources in html/resources/
    let shared_path = state.settings.html_dir.join("resources").join(&resource_path);
    if shared_path.exists() {
        if let Ok(data) = std::fs::read(&shared_path) {
            let content_type = mime_guess::from_path(&shared_path)
                .first_or_octet_stream()
                .to_string();
            return HttpResponse::Ok()
                .content_type(content_type)
                .insert_header(("Cache-Control", "public, max-age=31536000"))
                .body(data);
        }
    }

    HttpResponse::NotFound().finish()
}

/// POST /effect/install - install a plugin package (tar.gz upload)
#[post("/effect/install")]
pub async fn effect_install(
    state: web::Data<AppState>,
    body: web::Bytes,
) -> HttpResponse {
    let tmp_dir = &state.settings.download_tmp_dir;
    let plugin_dir = &state.settings.lv2_plugin_dir;

    // Ensure tmp dir exists
    let _ = std::fs::create_dir_all(tmp_dir);

    // Save uploaded archive
    let archive_name = format!("upload-{}.tar.gz", rand::random::<u64>());
    let archive_path = tmp_dir.join(&archive_name);
    if let Err(e) = std::fs::write(&archive_path, &body) {
        return HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({"ok": false, "error": format!("Failed to save archive: {}", e)}));
    }

    // Extract archive
    let tar_result = tokio::process::Command::new("tar")
        .args(["zxf", &archive_path.to_string_lossy()])
        .current_dir(tmp_dir)
        .output()
        .await;

    let _ = std::fs::remove_file(&archive_path);

    if let Err(e) = tar_result {
        return HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({"ok": false, "error": format!("Failed to extract: {}", e)}));
    }

    // Find and install extracted bundles
    let mut installed: Vec<String> = Vec::new();
    let mut removed: Vec<String> = Vec::new();
    let mut error = String::new();

    let entries: Vec<_> = match std::fs::read_dir(tmp_dir) {
        Ok(e) => e.filter_map(|e| e.ok()).collect(),
        Err(_) => Vec::new(),
    };

    let mut session = state.session.write().await;

    for entry in entries {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !entry.path().is_dir() {
            continue;
        }

        let tmp_path = entry.path();
        let bundle_path = plugin_dir.join(&*name_str);

        // Remove existing bundle if present
        if bundle_path.exists() {
            let bp_str = bundle_path.to_string_lossy().to_string();
            if crate::lv2_utils::is_bundle_loaded(&bp_str) {
                // Remove from mod-host
                session.host.ipc.send_notmodified(
                    &format!("bundle_remove \"{}\"", bp_str.replace('"', "\\\"")),
                    None,
                    "boolean",
                ).await;
                let rm_plugins = crate::lv2_utils::remove_bundle_from_lilv_world(&bp_str, None);
                removed.extend(rm_plugins);
            }
            let _ = std::fs::remove_dir_all(&bundle_path);
        }

        // Move bundle to plugin dir
        if let Err(e) = std::fs::rename(&tmp_path, &bundle_path) {
            // rename may fail across filesystems, try copy
            if let Err(e2) = copy_dir_recursive(&tmp_path, &bundle_path) {
                error = format!("Failed to install {}: {} / {}", name_str, e, e2);
                break;
            }
            let _ = std::fs::remove_dir_all(&tmp_path);
        }

        // Add to mod-host and lilv
        let bp_str = bundle_path.to_string_lossy().to_string();
        session.host.ipc.send_notmodified(
            &format!("bundle_add \"{}\"", bp_str.replace('"', "\\\"")),
            None,
            "boolean",
        ).await;
        let new_plugins = crate::lv2_utils::add_bundle_to_lilv_world(&bp_str);
        installed.extend(new_plugins);
    }

    // Clean up remaining tmp files
    if let Ok(entries) = std::fs::read_dir(tmp_dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }

    crate::lv2_utils::reset_get_all_pedalboards_cache(crate::lv2_utils::PEDALBOARD_INFO_BOTH);
    crate::utils::os_sync();

    // Refresh plugin cache after install
    state.plugin_cache.refresh();

    if !error.is_empty() || installed.is_empty() {
        HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({
                "ok": false,
                "error": if error.is_empty() { "No plugins found in bundle".to_string() } else { error },
                "removed": removed,
                "installed": [],
            }))
    } else {
        HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({
                "ok": true,
                "removed": removed,
                "installed": installed,
            }))
    }
}

/// Recursively copy a directory.
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let dest_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else {
            std::fs::copy(entry.path(), dest_path)?;
        }
    }
    Ok(())
}

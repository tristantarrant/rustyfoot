// Pedalboard management endpoints, ported from webserver.py
// /pedalboard/list, /pedalboard/save, /pedalboard/load_bundle, /pedalboard/remove,
// /pedalboard/rename, /pedalboard/image/*, /pedalboard/info, /pedalboard/transport/*

use actix_web::{get, post, web, HttpResponse};
use serde::Deserialize;
use serde_json::json;

use crate::lv2_utils;
use crate::AppState;

/// GET /pedalboard/list - list all user pedalboards
#[get("/pedalboard/list")]
pub async fn pedalboard_list(_state: web::Data<AppState>) -> HttpResponse {
    let pedalboards = lv2_utils::get_all_pedalboards(lv2_utils::PEDALBOARD_INFO_USER_ONLY);
    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(pedalboards)
}

#[derive(Deserialize)]
pub struct SaveForm {
    title: Option<String>,
    #[serde(rename = "asNew")]
    as_new: Option<String>,
}

/// POST /pedalboard/save - save current pedalboard
#[post("/pedalboard/save")]
pub async fn pedalboard_save(
    form: web::Form<SaveForm>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let title = form
        .title
        .as_deref()
        .unwrap_or(&state.settings.untitled_pedalboard_name);
    let as_new = form
        .as_new
        .as_deref()
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false);

    let mut session = state.session.write().await;
    let (ok, bundlepath, new_title) = session
        .web_save_pedalboard(title, as_new, &state.settings)
        .await;

    if ok {
        let final_title = new_title.unwrap_or_else(|| title.to_string());
        HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({
                "ok": true,
                "bundlepath": bundlepath,
                "title": final_title,
            }))
    } else {
        HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({"ok": false, "error": "save failed"}))
    }
}

#[derive(Deserialize)]
pub struct BundleQuery {
    bundlepath: Option<String>,
}

#[derive(Deserialize)]
pub struct LoadBundleForm {
    bundlepath: Option<String>,
    #[serde(rename = "isDefault")]
    is_default: Option<String>,
}

/// POST /pedalboard/load_bundle - load a pedalboard from disk
#[post("/pedalboard/load_bundle")]
pub async fn pedalboard_load_bundle(
    form: web::Form<LoadBundleForm>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let bundlepath = form.bundlepath.as_deref().unwrap_or("");
    if bundlepath.is_empty() {
        return HttpResponse::BadRequest()
            .json(json!({"ok": false, "error": "missing bundlepath"}));
    }
    let is_default = form
        .is_default
        .as_deref()
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false);

    let midi_cal = state.midi_calibration.read().unwrap().clone();
    let mut session = state.session.write().await;
    match session
        .web_load_pedalboard(bundlepath, is_default, &state.settings, &midi_cal)
        .await
    {
        Some(name) => HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({"ok": true, "name": name})),
        None => HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({"ok": false})),
    }
}

/// GET /pedalboard/info - get pedalboard metadata
#[get("/pedalboard/info")]
pub async fn pedalboard_info(
    query: web::Query<BundleQuery>,
    _state: web::Data<AppState>,
) -> HttpResponse {
    let bundlepath = query.bundlepath.as_deref().unwrap_or("");
    if bundlepath.is_empty() {
        return HttpResponse::BadRequest().json(json!({}));
    }

    match lv2_utils::get_pedalboard_info(bundlepath) {
        Some(info) => HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({
                "title": info.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                "width": info.get("width").and_then(|v| v.as_i64()).unwrap_or(0),
                "height": info.get("height").and_then(|v| v.as_i64()).unwrap_or(0),
                "version": info.get("version").and_then(|v| v.as_u64()).unwrap_or(0),
            })),
        None => HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({})),
    }
}

/// GET /pedalboard/remove - remove a pedalboard
#[get("/pedalboard/remove")]
pub async fn pedalboard_remove(
    query: web::Query<BundleQuery>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let bundlepath = query.bundlepath.as_deref().unwrap_or("");
    if bundlepath.is_empty() {
        return HttpResponse::BadRequest().json(false);
    }

    let path = std::path::Path::new(bundlepath);
    if path.is_dir() && bundlepath.ends_with(".pedalboard") {
        tracing::info!("[pedalboard] deleting {}", bundlepath);
        if let Err(e) = std::fs::remove_dir_all(path) {
            tracing::error!("[pedalboard] failed to remove {}: {}", bundlepath, e);
            return HttpResponse::Ok()
                .insert_header(("Cache-Control", "no-store"))
                .json(false);
        }
        // Reset cache
        lv2_utils::reset_get_all_pedalboards_cache(lv2_utils::PEDALBOARD_INFO_USER_ONLY);

        // Notify HMI to reload pedalboard list
        let session = state.session.read().await;
        session.hmi.send(crate::mod_protocol::CMD_PEDALBOARD_RELOAD_LIST, None, "boolean");
    }

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(true)
}

#[derive(Deserialize)]
pub struct RenameQuery {
    bundlepath: Option<String>,
    title: Option<String>,
}

/// POST /pedalboard/rename - rename a pedalboard
#[post("/pedalboard/rename")]
pub async fn pedalboard_rename(
    query: web::Query<RenameQuery>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let bundlepath = query.bundlepath.as_deref().unwrap_or("");
    let new_title = query.title.as_deref().unwrap_or("");

    if bundlepath.is_empty() || new_title.is_empty() {
        return HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({"ok": false}));
    }

    // Re-read pedalboard info and rewrite the TTL with the new title
    if let Some(info) = lv2_utils::get_pedalboard_info(bundlepath) {
        let old_title = info.get("title").and_then(|v| v.as_str()).unwrap_or("");
        if old_title == new_title {
            return HttpResponse::Ok()
                .insert_header(("Cache-Control", "no-store"))
                .json(json!({"ok": true}));
        }

        // Find and update the TTL file — replace the doap:name line
        let bundle_dir = std::path::Path::new(bundlepath);
        if let Ok(entries) = std::fs::read_dir(bundle_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("ttl")
                    && path.file_name().and_then(|n| n.to_str()) != Some("manifest.ttl")
                {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let escaped_old = old_title.replace('"', "\\\"");
                        let escaped_new = new_title.replace('"', "\\\"");
                        let new_content = content.replace(
                            &format!("doap:name \"{}\"", escaped_old),
                            &format!("doap:name \"{}\"", escaped_new),
                        );
                        if new_content != content {
                            let _ = crate::utils::text_file_flusher(&path, &new_content);
                        }
                    }
                }
            }
        }

        // Update in-memory state if this is the currently loaded pedalboard
        let mut session = state.session.write().await;
        if session.host.pedalboard.path.to_string_lossy() == bundlepath {
            session.host.pedalboard.name = new_title.to_string();
        }

        // Reset cache
        lv2_utils::reset_get_all_pedalboards_cache(lv2_utils::PEDALBOARD_INFO_USER_ONLY);

        HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({"ok": true}))
    } else {
        HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({"ok": false}))
    }
}

/// GET /pedalboard/image/{type}.png - get pedalboard screenshot/thumbnail
#[get("/pedalboard/image/{imgtype}.png")]
pub async fn pedalboard_image(
    _path: web::Path<String>,
    query: web::Query<BundleQuery>,
) -> HttpResponse {
    let bundlepath = query.bundlepath.as_deref().unwrap_or("");
    if bundlepath.is_empty() {
        return HttpResponse::BadRequest().finish();
    }

    let img_path = std::path::Path::new(bundlepath).join("screenshot.png");
    if img_path.exists() {
        match std::fs::read(&img_path) {
            Ok(data) => HttpResponse::Ok()
                .content_type("image/png")
                .insert_header(("Cache-Control", "public, max-age=31536000"))
                .body(data),
            Err(_) => HttpResponse::NotFound().finish(),
        }
    } else {
        HttpResponse::NotFound().finish()
    }
}

/// GET /pedalboard/image/wait - wait for screenshot generation to finish
#[get("/pedalboard/image/wait")]
pub async fn pedalboard_image_wait(
    query: web::Query<BundleQuery>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let bundlepath = query.bundlepath.as_deref().unwrap_or("");
    if bundlepath.is_empty() {
        return HttpResponse::BadRequest().json(json!({"ok": false}));
    }

    let generator = {
        let session = state.session.read().await;
        session.screenshot_generator.clone()
    };
    let ok = generator.wait_for_screenshot(std::path::Path::new(bundlepath)).await;

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(json!({"ok": ok}))
}

/// GET /pedalboard/image/generate - schedule screenshot generation
#[get("/pedalboard/image/generate")]
pub async fn pedalboard_image_generate(
    query: web::Query<BundleQuery>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let bundlepath = query.bundlepath.as_deref().unwrap_or("");
    if bundlepath.is_empty() {
        return HttpResponse::BadRequest().json(json!({"ok": false}));
    }

    let generator = {
        let session = state.session.read().await;
        session.screenshot_generator.clone()
    };
    generator.schedule_screenshot(std::path::Path::new(bundlepath));

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(json!({"ok": true}))
}

/// GET /pedalboard/image/check - check if screenshot exists
#[get("/pedalboard/image/check")]
pub async fn pedalboard_image_check(
    query: web::Query<BundleQuery>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let bundlepath = query.bundlepath.as_deref().unwrap_or("");
    if bundlepath.is_empty() {
        return HttpResponse::BadRequest().json(json!({"status": -1}));
    }

    let generator = {
        let session = state.session.read().await;
        session.screenshot_generator.clone()
    };
    let status = generator
        .check_screenshot(std::path::Path::new(bundlepath))
        .await;

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "public, max-age=31536000"))
        .json(json!({"status": status}))
}

#[derive(Deserialize)]
pub struct CvForm {
    uri: Option<String>,
    name: Option<String>,
}

/// POST /pedalboard/transport/set_sync_mode/{mode} - set transport sync mode
#[post("/pedalboard/transport/set_sync_mode/{mode}")]
pub async fn pedalboard_transport_set_sync_mode(
    path: web::Path<String>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let mode = path.into_inner();
    let mut session = state.session.write().await;
    session.host.transport.sync = crate::host::transport::SyncMode::from_str(&mode);
    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(true)
}

/// POST /pedalboard/cv_addressing_plugin_port/add - add CV addressing port
#[post("/pedalboard/cv_addressing_plugin_port/add")]
pub async fn pedalboard_cv_add(
    form: web::Form<CvForm>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let uri = form.uri.as_deref().unwrap_or("");
    let name = form.name.as_deref().unwrap_or("");
    if uri.is_empty() {
        return HttpResponse::BadRequest().json(json!({"ok": false}));
    }

    let mut session = state.session.write().await;
    let op_mode = session.host.cv_addressing_plugin_port_add(uri, name);

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(json!({"ok": true, "operational_mode": op_mode}))
}

/// POST /pedalboard/cv_addressing_plugin_port/remove - remove CV addressing port
#[post("/pedalboard/cv_addressing_plugin_port/remove")]
pub async fn pedalboard_cv_remove(
    form: web::Form<CvForm>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let uri = form.uri.as_deref().unwrap_or("");
    if uri.is_empty() {
        return HttpResponse::BadRequest().json(false);
    }

    let mut session = state.session.write().await;
    let ok = session.host.cv_addressing_plugin_port_remove(uri);

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(ok)
}

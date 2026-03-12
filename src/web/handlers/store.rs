// Store endpoints for browsing and installing plugins/content from external sources.

use actix_web::{get, post, web, HttpRequest, HttpResponse};
use serde::Deserialize;
use serde_json::json;

use crate::store::{self, StoreQuery};
use crate::AppState;

/// GET /store/sources - list available store backends
#[get("/store/sources")]
pub async fn store_sources(
    state: web::Data<AppState>,
) -> HttpResponse {
    let tone3000_auth = state.store_tone3000.is_authenticated().await;
    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(json!({
            "target": store::detect_target(),
            "sources": store::SOURCES,
            "auth": {
                "tone3000": tone3000_auth,
            },
        }))
}

/// GET /store/{source}/search - search/browse items
#[get("/store/{source}/search")]
pub async fn store_search(
    path: web::Path<String>,
    query: web::Query<StoreQuery>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let source = path.into_inner();
    let result = match source.as_str() {
        "patchstorage" => state.store_patchstorage.search(&query).await,
        "tone3000" => state.store_tone3000.search(&query).await,
        _ => Err(format!("unknown store source: {}", source)),
    };

    match result {
        Ok(data) => HttpResponse::Ok()
            .insert_header(("Cache-Control", "max-age=300"))
            .json(data),
        Err(e) => HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({"ok": false, "error": e})),
    }
}

/// GET /store/{source}/get/{id} - get item details
#[get("/store/{source}/get/{id}")]
pub async fn store_get(
    path: web::Path<(String, u64)>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let (source, id) = path.into_inner();
    let result = match source.as_str() {
        "patchstorage" => state.store_patchstorage.get(id).await,
        "tone3000" => state.store_tone3000.get(id).await,
        _ => Err(format!("unknown store source: {}", source)),
    };

    match result {
        Ok(data) => HttpResponse::Ok()
            .insert_header(("Cache-Control", "max-age=300"))
            .json(data),
        Err(e) => HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({"ok": false, "error": e})),
    }
}

/// GET /store/{source}/categories - list available categories
#[get("/store/{source}/categories")]
pub async fn store_categories(
    path: web::Path<String>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let source = path.into_inner();
    let result = match source.as_str() {
        "patchstorage" => state.store_patchstorage.categories().await,
        "tone3000" => state.store_tone3000.categories().await,
        _ => Err(format!("unknown store source: {}", source)),
    };

    match result {
        Ok(data) => HttpResponse::Ok()
            .insert_header(("Cache-Control", "max-age=3600"))
            .json(data),
        Err(e) => HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({"ok": false, "error": e})),
    }
}

/// Metadata sent from the frontend for Tone3000 installs.
#[derive(Deserialize, Default)]
pub struct Tone3000InstallMeta {
    #[serde(default)]
    title: String,
    #[serde(default)]
    categories: Vec<String>,
}

/// POST /store/{source}/install/{id} - download and install an item
#[post("/store/{source}/install/{id}")]
pub async fn store_install(
    path: web::Path<(String, u64)>,
    body: Option<web::Json<Tone3000InstallMeta>>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let (source, id) = path.into_inner();

    match source.as_str() {
        "patchstorage" => install_patchstorage(id, &state).await,
        "tone3000" => {
            let meta = body.map(|b| b.into_inner()).unwrap_or_default();
            install_tone3000(id, &meta, &state).await
        }
        _ => HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({"ok": false, "error": format!("unknown store source: {}", source)})),
    }
}

/// Download an LV2 plugin from Patchstorage and install it.
async fn install_patchstorage(id: u64, state: &web::Data<AppState>) -> HttpResponse {
    // Get patch details to find the right file
    let patch = match state.store_patchstorage.get(id).await {
        Ok(p) => p,
        Err(e) => return HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({"ok": false, "error": e})),
    };

    // Find the file for our target
    let file = match patch.files.first() {
        Some(f) => f,
        None => return HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({"ok": false, "error": "No compatible file found for this target"})),
    };

    tracing::info!("[store] downloading {} ({} bytes) from patchstorage", file.filename, file.filesize);

    // Download the file
    let data = match state.store_patchstorage.download(id, file.id).await {
        Ok(d) => d,
        Err(e) => return HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({"ok": false, "error": e})),
    };

    tracing::info!("[store] downloaded {} bytes, installing", data.len());

    // Install using the same logic as effect_install
    let tmp_dir = &state.settings.download_tmp_dir;
    let plugin_dir = &state.settings.lv2_plugin_dir;
    let _ = std::fs::create_dir_all(tmp_dir);

    // Save archive
    let archive_path = tmp_dir.join(&file.filename);
    if let Err(e) = std::fs::write(&archive_path, &data) {
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
        tracing::info!("[store] installed plugins: {:?}", installed);
        HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({
                "ok": true,
                "removed": removed,
                "installed": installed,
            }))
    }
}

/// Download a NAM model / IR from Tone3000 and save to user files.
async fn install_tone3000(id: u64, meta: &Tone3000InstallMeta, state: &web::Data<AppState>) -> HttpResponse {
    // Get models for this tone
    let tone = match state.store_tone3000.get(id).await {
        Ok(t) => t,
        Err(e) => return HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({"ok": false, "error": e})),
    };

    if tone.files.is_empty() {
        return HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({"ok": false, "error": "No downloadable models found for this tone"}));
    }

    // Determine destination directory based on categories from frontend metadata
    let is_ir = meta.categories.iter().any(|c| c.contains("Impulse"));
    let dest_subdir = if is_ir { "Speaker Cabinets IRs" } else { "NAM Models" };
    let dest_dir = state.settings.user_files_dir.join(dest_subdir);
    if let Err(e) = std::fs::create_dir_all(&dest_dir) {
        return HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({"ok": false, "error": format!("Failed to create {}: {}", dest_dir.display(), e)}));
    }

    // Create a subdirectory for this tone
    let title = if meta.title.is_empty() { format!("tone-{}", id) } else { meta.title.clone() };
    let safe_title = title.replace(|c: char| !c.is_alphanumeric() && c != ' ' && c != '-' && c != '_', "");
    let tone_dir = dest_dir.join(&safe_title);
    if let Err(e) = std::fs::create_dir_all(&tone_dir) {
        return HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({"ok": false, "error": format!("Failed to create {}: {}", tone_dir.display(), e)}));
    }

    let mut installed: Vec<String> = Vec::new();
    let mut error = String::new();

    for (i, file) in tone.files.iter().enumerate() {
        if file.url.is_empty() {
            continue;
        }

        // Pace downloads to avoid rate limiting
        if i > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        tracing::info!("[store] downloading {} from tone3000 ({}/{})", file.filename, i + 1, tone.files.len());

        let data = match state.store_tone3000.download_model(&file.url).await {
            Ok(d) => d,
            Err(e) => {
                error = format!("Failed to download {}: {}", file.filename, e);
                break;
            }
        };

        // Determine filename and extension
        let ext_from_url = file.url.rsplit('.').next()
            .filter(|e| e.len() <= 4 && e.chars().all(|c| c.is_alphanumeric()))
            .unwrap_or(if is_ir { "wav" } else { "nam" });
        let raw_name = if file.filename.is_empty() {
            format!("{}.{}", file.id, ext_from_url)
        } else if file.filename.contains('.') {
            file.filename.clone()
        } else {
            format!("{}.{}", file.filename, ext_from_url)
        };
        let filename = raw_name.replace(|c: char| !c.is_alphanumeric() && c != ' ' && c != '-' && c != '_' && c != '.', "");

        let dest_path = tone_dir.join(&filename);
        if let Err(e) = std::fs::write(&dest_path, &data) {
            error = format!("Failed to save {}: {}", filename, e);
            break;
        }

        tracing::info!("[store] saved {} ({} bytes)", dest_path.display(), data.len());
        installed.push(filename);
    }

    if !error.is_empty() || installed.is_empty() {
        HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({
                "ok": false,
                "error": if error.is_empty() { "No models were downloaded".to_string() } else { error },
            }))
    } else {
        HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({
                "ok": true,
                "installed": installed,
                "directory": format!("{}/{}", dest_subdir, safe_title),
            }))
    }
}

/// GET /store/tone3000/auth/status - check auth status
#[get("/store/tone3000/auth/status")]
pub async fn tone3000_auth_status(
    state: web::Data<AppState>,
) -> HttpResponse {
    let authenticated = state.store_tone3000.is_authenticated().await;
    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(json!({ "authenticated": authenticated }))
}

/// GET /store/tone3000/auth/start - redirect to Tone3000 OAuth
#[get("/store/tone3000/auth/start")]
pub async fn tone3000_auth_start(
    req: HttpRequest,
) -> HttpResponse {
    let host = req.headers()
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("localhost:8888");
    let hostname = host.split(':').next().unwrap_or(host);
    let scheme = if hostname == "localhost"
        || hostname.starts_with("127.")
        || hostname.starts_with("10.")
        || hostname.starts_with("192.168.")
        || hostname.starts_with("172.")
    { "http" } else { "https" };
    let callback_url = format!("{}://{}/store/tone3000/auth/callback", scheme, host);
    let encoded = urlencoding::encode(&callback_url);

    let auth_url = format!(
        "https://www.tone3000.com/api/v1/auth?redirect_url={}",
        encoded
    );

    HttpResponse::Found()
        .insert_header(("Location", auth_url))
        .finish()
}

#[derive(Deserialize)]
pub struct AuthCallbackQuery {
    api_key: String,
}

/// GET /store/tone3000/auth/callback - receive API key from OAuth redirect
#[get("/store/tone3000/auth/callback")]
pub async fn tone3000_auth_callback(
    query: web::Query<AuthCallbackQuery>,
    state: web::Data<AppState>,
) -> HttpResponse {
    match state.store_tone3000.create_session(&query.api_key).await {
        Ok(()) => {
            // Redirect back to the store page with a success indicator
            HttpResponse::Found()
                .insert_header(("Location", "/?tone3000_auth=ok"))
                .finish()
        }
        Err(e) => {
            tracing::error!("[store] tone3000 auth failed: {}", e);
            HttpResponse::Found()
                .insert_header(("Location", format!("/?tone3000_auth=error&message={}", urlencoding::encode(&e))))
                .finish()
        }
    }
}

/// POST /store/tone3000/auth/disconnect - clear stored tokens
#[post("/store/tone3000/auth/disconnect")]
pub async fn tone3000_auth_disconnect(
    state: web::Data<AppState>,
) -> HttpResponse {
    state.store_tone3000.disconnect().await;
    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(json!({"ok": true}))
}

fn copy_dir_recursive(src: &std::path::Path, dest: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let entries = std::fs::read_dir(src).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let target = dest.join(entry.file_name());
        let ft = entry.file_type().map_err(|e| e.to_string())?;
        if ft.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else if ft.is_file() {
            std::fs::copy(entry.path(), &target).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

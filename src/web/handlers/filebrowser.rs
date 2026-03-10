// File browser endpoints for managing user files
// Replaces the external Cloud9 file manager

use actix_multipart::Multipart;
use actix_web::{delete, get, post, web, HttpResponse};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};

use crate::AppState;

/// Resolve a relative path within user_files_dir, preventing path traversal.
fn resolve_safe_path(base: &Path, rel: &str) -> Option<PathBuf> {
    let rel = rel.trim_start_matches('/');
    let candidate = base.join(rel);
    let canonical_base = base.canonicalize().ok()?;
    // For new paths (mkdir, upload), parent must exist within base
    let check = if candidate.exists() {
        candidate.canonicalize().ok()?
    } else {
        let parent = candidate.parent()?.canonicalize().ok()?;
        if !parent.starts_with(&canonical_base) {
            return None;
        }
        parent.join(candidate.file_name()?)
    };
    if check.starts_with(&canonical_base) {
        Some(candidate)
    } else {
        None
    }
}

#[derive(Deserialize)]
pub struct PathQuery {
    path: Option<String>,
}

#[derive(Deserialize)]
pub struct RenameForm {
    from: String,
    to: String,
}

#[derive(Deserialize)]
pub struct PathForm {
    path: String,
}

/// GET /filebrowser - serve the file browser HTML page
#[get("/filebrowser")]
pub async fn filebrowser_page() -> HttpResponse {
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(include_str!("filebrowser.html"))
}

/// GET /filebrowser/api/list - list directory contents
#[get("/filebrowser/api/list")]
pub async fn filebrowser_list(
    query: web::Query<PathQuery>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let base = &state.settings.user_files_dir;
    let rel = query.path.as_deref().unwrap_or("");
    let dir = match resolve_safe_path(base, rel) {
        Some(p) => p,
        None => return HttpResponse::Forbidden().json(json!({"error": "Invalid path"})),
    };

    if !dir.is_dir() {
        return HttpResponse::NotFound().json(json!({"error": "Directory not found"}));
    }

    let mut entries = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(&dir) {
        for entry in read_dir.flatten() {
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = meta.is_dir();
            let size = if is_dir { 0 } else { meta.len() };
            entries.push(json!({
                "name": name,
                "is_dir": is_dir,
                "size": size,
            }));
        }
    }

    entries.sort_by(|a, b| {
        let a_dir = a["is_dir"].as_bool().unwrap_or(false);
        let b_dir = b["is_dir"].as_bool().unwrap_or(false);
        b_dir.cmp(&a_dir).then_with(|| {
            let an = a["name"].as_str().unwrap_or("");
            let bn = b["name"].as_str().unwrap_or("");
            an.to_lowercase().cmp(&bn.to_lowercase())
        })
    });

    HttpResponse::Ok().json(json!({
        "path": rel,
        "entries": entries,
    }))
}

/// POST /filebrowser/api/upload - upload files (multipart)
#[post("/filebrowser/api/upload")]
pub async fn filebrowser_upload(
    query: web::Query<PathQuery>,
    state: web::Data<AppState>,
    mut payload: Multipart,
) -> HttpResponse {
    let base = &state.settings.user_files_dir;
    let rel = query.path.as_deref().unwrap_or("");
    let dir = match resolve_safe_path(base, rel) {
        Some(p) => p,
        None => return HttpResponse::Forbidden().json(json!({"error": "Invalid path"})),
    };

    if !dir.is_dir() {
        return HttpResponse::BadRequest().json(json!({"error": "Target directory not found"}));
    }

    let mut uploaded = Vec::new();

    while let Some(Ok(mut field)) = payload.next().await {
        let filename = match field.content_disposition().and_then(|cd| cd.get_filename().map(String::from)) {
            Some(f) => sanitize_filename(&f),
            None => continue,
        };
        if filename.is_empty() {
            continue;
        }

        let filepath = dir.join(&filename);
        // Safety check: ensure we're still within base
        if resolve_safe_path(base, &filepath.strip_prefix(base).unwrap_or(Path::new("")).to_string_lossy()).is_none() {
            continue;
        }

        let mut data = Vec::new();
        while let Some(Ok(chunk)) = field.next().await {
            data.extend_from_slice(&chunk);
        }

        if let Err(e) = std::fs::write(&filepath, &data) {
            return HttpResponse::InternalServerError()
                .json(json!({"error": format!("Write failed: {}", e)}));
        }
        uploaded.push(filename);
    }

    HttpResponse::Ok().json(json!({"ok": true, "uploaded": uploaded}))
}

/// GET /filebrowser/api/download - download a file
#[get("/filebrowser/api/download")]
pub async fn filebrowser_download(
    query: web::Query<PathQuery>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let base = &state.settings.user_files_dir;
    let rel = match &query.path {
        Some(p) => p.as_str(),
        None => return HttpResponse::BadRequest().json(json!({"error": "Missing path"})),
    };
    let filepath = match resolve_safe_path(base, rel) {
        Some(p) => p,
        None => return HttpResponse::Forbidden().json(json!({"error": "Invalid path"})),
    };

    if !filepath.is_file() {
        return HttpResponse::NotFound().json(json!({"error": "File not found"}));
    }

    let data = match std::fs::read(&filepath) {
        Ok(d) => d,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(json!({"error": format!("Read failed: {}", e)}))
        }
    };

    let filename = filepath
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let mime = mime_guess::from_path(&filepath)
        .first_or_octet_stream()
        .to_string();

    HttpResponse::Ok()
        .content_type(mime)
        .insert_header((
            "Content-Disposition",
            format!("attachment; filename=\"{}\"", filename),
        ))
        .body(data)
}

/// POST /filebrowser/api/mkdir - create a directory
#[post("/filebrowser/api/mkdir")]
pub async fn filebrowser_mkdir(
    form: web::Json<PathForm>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let base = &state.settings.user_files_dir;
    let dir = match resolve_safe_path(base, &form.path) {
        Some(p) => p,
        None => return HttpResponse::Forbidden().json(json!({"error": "Invalid path"})),
    };

    if dir.exists() {
        return HttpResponse::Conflict().json(json!({"error": "Already exists"}));
    }

    match std::fs::create_dir_all(&dir) {
        Ok(_) => HttpResponse::Ok().json(json!({"ok": true})),
        Err(e) => HttpResponse::InternalServerError()
            .json(json!({"error": format!("mkdir failed: {}", e)})),
    }
}

/// DELETE /filebrowser/api/delete - delete a file or directory
#[delete("/filebrowser/api/delete")]
pub async fn filebrowser_delete(
    form: web::Json<PathForm>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let base = &state.settings.user_files_dir;
    let target = match resolve_safe_path(base, &form.path) {
        Some(p) => p,
        None => return HttpResponse::Forbidden().json(json!({"error": "Invalid path"})),
    };

    // Don't allow deleting the root user files dir itself
    if target.canonicalize().ok() == base.canonicalize().ok() {
        return HttpResponse::Forbidden().json(json!({"error": "Cannot delete root directory"}));
    }

    if !target.exists() {
        return HttpResponse::NotFound().json(json!({"error": "Not found"}));
    }

    let result = if target.is_dir() {
        std::fs::remove_dir_all(&target)
    } else {
        std::fs::remove_file(&target)
    };

    match result {
        Ok(_) => HttpResponse::Ok().json(json!({"ok": true})),
        Err(e) => HttpResponse::InternalServerError()
            .json(json!({"error": format!("Delete failed: {}", e)})),
    }
}

/// POST /filebrowser/api/rename - rename or move a file/directory
#[post("/filebrowser/api/rename")]
pub async fn filebrowser_rename(
    form: web::Json<RenameForm>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let base = &state.settings.user_files_dir;
    let from = match resolve_safe_path(base, &form.from) {
        Some(p) => p,
        None => return HttpResponse::Forbidden().json(json!({"error": "Invalid source path"})),
    };
    let to = match resolve_safe_path(base, &form.to) {
        Some(p) => p,
        None => return HttpResponse::Forbidden().json(json!({"error": "Invalid target path"})),
    };

    if !from.exists() {
        return HttpResponse::NotFound().json(json!({"error": "Source not found"}));
    }
    if to.exists() {
        return HttpResponse::Conflict().json(json!({"error": "Target already exists"}));
    }

    match std::fs::rename(&from, &to) {
        Ok(_) => HttpResponse::Ok().json(json!({"ok": true})),
        Err(e) => HttpResponse::InternalServerError()
            .json(json!({"error": format!("Rename failed: {}", e)})),
    }
}

fn sanitize_filename(name: &str) -> String {
    let name = Path::new(name)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    // Remove any remaining path separators
    name.replace(['/', '\\'], "_")
}

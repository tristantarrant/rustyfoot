// Recording/playback endpoints, ported from webserver.py
// /recording/start, /recording/stop, /recording/reset, /recording/play/*, /recording/download

use actix_web::{get, web, HttpResponse};
use serde_json::json;

use crate::AppState;

/// GET /recording/start - start recording audio
#[get("/recording/start")]
pub async fn recording_start(state: web::Data<AppState>) -> HttpResponse {
    let mut session = state.session.write().await;
    session.web_recording_start();
    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(true)
}

/// GET /recording/stop - stop recording audio
#[get("/recording/stop")]
pub async fn recording_stop(state: web::Data<AppState>) -> HttpResponse {
    let mut session = state.session.write().await;
    session.web_recording_stop();
    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(true)
}

/// GET /recording/reset - delete recording
#[get("/recording/reset")]
pub async fn recording_reset(state: web::Data<AppState>) -> HttpResponse {
    let mut session = state.session.write().await;
    session.web_recording_delete();
    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(true)
}

/// GET /recording/play/{action} - control playback (start/stop/wait)
#[get("/recording/play/{action}")]
pub async fn recording_play(
    path: web::Path<String>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let action = path.into_inner();

    match action.as_str() {
        "start" => {
            let mut session = state.session.write().await;
            let ok = session.web_playing_start();
            HttpResponse::Ok()
                .insert_header(("Cache-Control", "no-store"))
                .json(ok)
        }
        "stop" => {
            let mut session = state.session.write().await;
            session.web_playing_stop();
            HttpResponse::Ok()
                .insert_header(("Cache-Control", "no-store"))
                .json(true)
        }
        "wait" => {
            let wait_handle = {
                let session = state.session.read().await;
                session.player.wait_handle()
            };
            wait_handle.notified().await;
            HttpResponse::Ok()
                .insert_header(("Cache-Control", "no-store"))
                .json(true)
        }
        _ => HttpResponse::NotFound().json(false),
    }
}

/// GET /recording/download - download recorded audio as base64
#[get("/recording/download")]
pub async fn recording_download(state: web::Data<AppState>) -> HttpResponse {
    use base64::Engine;

    let capture_path = &state.settings.capture_path;

    if capture_path.exists() {
        match std::fs::read(capture_path) {
            Ok(data) => {
                let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
                HttpResponse::Ok()
                    .insert_header(("Cache-Control", "no-store"))
                    .json(json!({"ok": true, "audio": b64}))
            }
            Err(_) => HttpResponse::Ok()
                .insert_header(("Cache-Control", "no-store"))
                .json(json!({"ok": false})),
        }
    } else {
        HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({"ok": false}))
    }
}

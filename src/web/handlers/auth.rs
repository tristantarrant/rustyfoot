// Authentication and token endpoints, ported from webserver.py
// /auth/nonce, /auth/token, /tokens/get, /tokens/delete, /tokens/save

use actix_web::{get, post, web, HttpResponse};
use serde_json::json;

use crate::communication::token;
use crate::AppState;

/// POST /auth/nonce - create authentication nonce
#[post("/auth/nonce")]
pub async fn auth_nonce(
    body: web::Json<serde_json::Value>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let nonce = body
        .get("nonce")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match token::create_token_message(&state.settings, nonce) {
        Ok(msg) => HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(msg),
        Err(e) => {
            tracing::error!("Failed to create token message: {}", e);
            HttpResponse::Ok()
                .insert_header(("Cache-Control", "no-store"))
                .json(json!({"message": ""}))
        }
    }
}

/// POST /auth/token - validate authentication token
#[post("/auth/token")]
pub async fn auth_token(
    body: web::Bytes,
    state: web::Data<AppState>,
) -> HttpResponse {
    let message = String::from_utf8_lossy(&body);

    match token::decode_and_decrypt(&state.settings, &message) {
        Ok(access_token) => HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({"access_token": access_token})),
        Err(e) => {
            tracing::error!("Failed to decode token: {}", e);
            HttpResponse::Ok()
                .insert_header(("Cache-Control", "no-store"))
                .json(json!({"access_token": ""}))
        }
    }
}

/// GET /tokens/get - get stored tokens
#[get("/tokens/get")]
pub async fn tokens_get(state: web::Data<AppState>) -> HttpResponse {
    let keys_path = std::path::Path::new(&state.settings.keys_path);
    let token_file = keys_path.join("tokens.json");

    let data: serde_json::Value = crate::utils::safe_json_load_value(
        &token_file,
        json!({"ok": false}),
    );

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(data)
}

/// GET /tokens/delete - delete stored tokens
#[get("/tokens/delete")]
pub async fn tokens_delete(state: web::Data<AppState>) -> HttpResponse {
    let keys_path = std::path::Path::new(&state.settings.keys_path);
    let token_file = keys_path.join("tokens.json");
    let _ = std::fs::remove_file(&token_file);

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(true)
}

/// POST /tokens/save - save tokens
#[post("/tokens/save")]
pub async fn tokens_save(
    body: web::Json<serde_json::Value>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let keys_path = std::path::Path::new(&state.settings.keys_path);
    let token_file = keys_path.join("tokens.json");

    let json = serde_json::to_string_pretty(&body.into_inner()).unwrap_or_default();
    match crate::utils::text_file_flusher(&token_file, &json) {
        Ok(()) => HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(true),
        Err(_) => HttpResponse::InternalServerError().json(false),
    }
}

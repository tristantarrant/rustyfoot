// Bank management endpoints, ported from webserver.py
// /banks, /banks/save

use actix_web::{get, post, web, HttpResponse};

use crate::bank;
use crate::AppState;

/// GET /banks - load all banks with pedalboard data
#[get("/banks")]
pub async fn bank_load(state: web::Data<AppState>) -> HttpResponse {
    let banks = bank::list_banks(
        &state.settings.user_banks_json_file,
        &[],    // no broken bundles
        true,   // user banks
        false,  // don't auto-save
    );

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(banks)
}

/// POST /banks/save - save bank configuration
/// Accepts JSON body regardless of Content-Type header (jQuery sends
/// application/x-www-form-urlencoded by default).
#[post("/banks/save")]
pub async fn bank_save(
    body: String,
    state: web::Data<AppState>,
) -> HttpResponse {
    let banks: Vec<bank::Bank> = match serde_json::from_str(&body) {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("[banks] failed to parse save body: {}", e);
            return HttpResponse::BadRequest()
                .json(serde_json::json!({"ok": false, "error": e.to_string()}));
        }
    };
    bank::save_banks(&state.settings.user_banks_json_file, &banks);

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(true)
}

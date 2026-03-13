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

    // Enrich pedalboards with metadata (version, factory) from pedalboard info
    let enriched: Vec<serde_json::Value> = banks.iter().map(|bank| {
        let pedalboards: Vec<serde_json::Value> = bank.pedalboards.iter().map(|pb| {
            let info = crate::lv2_utils::get_pedalboard_info(&pb.bundle);
            let version = info.as_ref()
                .and_then(|i| i.get("version"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let factory = info.as_ref()
                .and_then(|i| i.get("factory"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            serde_json::json!({
                "title": pb.title,
                "bundle": pb.bundle,
                "version": version,
                "factory": factory,
            })
        }).collect();
        serde_json::json!({
            "title": bank.title,
            "pedalboards": pedalboards,
        })
    }).collect();

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(enriched)
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

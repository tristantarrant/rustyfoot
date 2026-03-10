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
#[post("/banks/save")]
pub async fn bank_save(
    body: web::Json<Vec<bank::Bank>>,
    state: web::Data<AppState>,
) -> HttpResponse {
    bank::save_banks(&state.settings.user_banks_json_file, &body);

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(true)
}

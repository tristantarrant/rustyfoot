use actix_web::{web, HttpResponse, get, post, delete};
use serde_json::json;
use crate::AppState;

/// GET /midi/calibration - get all MIDI CC calibration entries
#[get("/midi/calibration")]
pub async fn midi_calibration_get(state: web::Data<AppState>) -> HttpResponse {
    let cal = state.midi_calibration.read().unwrap();
    let mut result = serde_json::Map::new();
    for (&cc, &(cc_min, cc_max)) in cal.get_all() {
        result.insert(cc.to_string(), json!({"cc_min": cc_min, "cc_max": cc_max}));
    }
    HttpResponse::Ok().json(serde_json::Value::Object(result))
}

/// POST /midi/calibration - set calibration for a CC
/// Body: {"cc": 11, "cc_min": 3, "cc_max": 125}
#[post("/midi/calibration")]
pub async fn midi_calibration_set(
    body: web::Json<serde_json::Value>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let cc = match body.get("cc").and_then(|v| v.as_i64()) {
        Some(v) => v as i32,
        None => return HttpResponse::BadRequest().json(json!({"error": "missing cc"})),
    };
    let cc_min = body.get("cc_min").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let cc_max = body.get("cc_max").and_then(|v| v.as_i64()).unwrap_or(127) as i32;

    if cc < 0 || cc > 127 || cc_min < 0 || cc_max > 127 || cc_min >= cc_max {
        return HttpResponse::BadRequest().json(json!({"error": "invalid range"}));
    }

    state.midi_calibration.write().unwrap().set(cc, cc_min, cc_max);
    HttpResponse::Ok().json(json!({"ok": true}))
}

/// DELETE /midi/calibration/{cc} - remove calibration for a CC
#[delete("/midi/calibration/{cc}")]
pub async fn midi_calibration_delete(
    path: web::Path<i32>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let cc = path.into_inner();
    state.midi_calibration.write().unwrap().remove(cc);
    HttpResponse::Ok().json(json!({"ok": true}))
}

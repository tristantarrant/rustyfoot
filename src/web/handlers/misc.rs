use actix_web::{get, web, HttpResponse};
use serde_json::json;

use crate::AppState;

/// GET /ping - connectivity check (pings HMI and measures round-trip)
#[get("/ping")]
pub async fn ping(state: web::Data<AppState>) -> HttpResponse {
    let start = std::time::Instant::now();

    let (tx, rx) = tokio::sync::oneshot::channel();
    let callback: crate::hmi::HmiCallback = Box::new(move |resp| {
        let online = matches!(resp, crate::protocol::RespValue::Bool(true));
        let _ = tx.send(online);
    });

    {
        let session = state.session.read().await;
        session.web_ping(callback);
    }

    let online = match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        rx,
    ).await {
        Ok(Ok(v)) => v,
        _ => true, // timeout or channel error — assume online (matches Python behavior)
    };

    let elapsed_ms = start.elapsed().as_millis() as u64;
    let resp = if online {
        json!({
            "ihm_online": true,
            "ihm_time": if elapsed_ms == 0 { 1 } else { elapsed_ms },
        })
    } else {
        json!({
            "ihm_online": false,
            "ihm_time": 0,
        })
    };

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(resp)
}

/// GET /hello - basic status check
#[get("/hello")]
pub async fn hello(state: web::Data<AppState>) -> HttpResponse {
    let session = state.session.read().await;
    let online = !session.websockets.is_empty();

    let resp = json!({
        "online": online,
        "version": state.settings.image_version,
    });
    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .insert_header(("Access-Control-Allow-Origin", "*"))
        .json(resp)
}

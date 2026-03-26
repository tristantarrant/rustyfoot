// Snapshot management endpoints, ported from webserver.py
// /snapshot/save, /snapshot/saveas, /snapshot/rename, /snapshot/remove,
// /snapshot/list, /snapshot/name, /snapshot/load

use actix_web::{get, post, web, HttpResponse};
use serde::Deserialize;
use serde_json::json;

use crate::AppState;

/// POST /snapshot/save - save current snapshot
#[post("/snapshot/save")]
pub async fn snapshot_save(state: web::Data<AppState>) -> HttpResponse {
    let mut session = state.session.write().await;
    let ok = session.host.snapshot_save();
    session.send_snapshot_list_to_hmi();
    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(ok)
}

#[derive(Deserialize)]
pub struct TitleQuery {
    title: Option<String>,
}

/// GET /snapshot/saveas - save as new snapshot
#[get("/snapshot/saveas")]
pub async fn snapshot_saveas(
    query: web::Query<TitleQuery>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let title = query
        .title
        .as_deref()
        .unwrap_or(&state.settings.default_snapshot_name);

    let mut session = state.session.write().await;
    let (id, final_title) = session.host.snapshot_saveas(title);
    session.send_snapshot_list_to_hmi();

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(json!({"ok": true, "id": id, "title": final_title}))
}

#[derive(Deserialize)]
pub struct IdTitleQuery {
    id: Option<i32>,
    title: Option<String>,
}

/// GET /snapshot/rename - rename a snapshot
#[get("/snapshot/rename")]
pub async fn snapshot_rename(
    query: web::Query<IdTitleQuery>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let id = query.id.unwrap_or(-1);
    let title = query.title.as_deref().unwrap_or("");

    if id < 0 || title.is_empty() {
        return HttpResponse::BadRequest().json(json!({"ok": false}));
    }

    let mut session = state.session.write().await;
    let ok = session.host.pedalboard.snapshot_rename(id, title);
    session.send_snapshot_list_to_hmi();

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(json!({"ok": ok, "title": title}))
}

#[derive(Deserialize)]
pub struct IdQuery {
    id: Option<i32>,
}

/// GET /snapshot/remove - delete a snapshot
#[get("/snapshot/remove")]
pub async fn snapshot_remove(
    query: web::Query<IdQuery>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let id = query.id.unwrap_or(-1);
    if id < 0 {
        return HttpResponse::BadRequest().json(false);
    }

    let mut session = state.session.write().await;
    let ok = session.host.pedalboard.snapshot_delete(id);
    session.send_snapshot_list_to_hmi();

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(ok)
}

/// GET /snapshot/list - list all snapshots
#[get("/snapshot/list")]
pub async fn snapshot_list(state: web::Data<AppState>) -> HttpResponse {
    let session = state.session.read().await;
    let mut result = serde_json::Map::new();

    for (i, snapshot) in session.host.pedalboard.snapshots.iter().enumerate() {
        if let Some(s) = snapshot {
            result.insert(i.to_string(), serde_json::Value::String(s.name.clone()));
        }
    }

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(result)
}

/// GET /snapshot/name - get snapshot name by ID
#[get("/snapshot/name")]
pub async fn snapshot_name(
    query: web::Query<IdQuery>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let id = query.id.unwrap_or(-1);
    let session = state.session.read().await;

    let name = if id >= 0 {
        session
            .host
            .pedalboard
            .snapshots
            .get(id as usize)
            .and_then(|s| s.as_ref())
            .map(|s| s.name.as_str())
    } else {
        None
    };

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(json!({"ok": name.is_some(), "name": name.unwrap_or("")}))
}

/// GET /snapshot/load - load a snapshot
#[get("/snapshot/load")]
pub async fn snapshot_load(
    query: web::Query<IdQuery>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let id = query.id.unwrap_or(-1);

    let mut session = state.session.write().await;
    let ws_broadcast = session.ws_broadcast.clone();
    let msg_cb = |msg: &str| {
        if let Some(ref tx) = ws_broadcast {
            let _ = tx.send(msg.to_string());
        }
    };

    let ok = session.host.snapshot_load(id, &msg_cb).await;
    session.send_snapshot_list_to_hmi();

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(ok)
}

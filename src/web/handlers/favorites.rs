// Favorites and preferences endpoints, ported from webserver.py
// /favorites/add, /favorites/remove, /config/set, /save_user_id

use actix_web::{post, web, HttpResponse};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::AppState;

#[derive(Deserialize)]
pub struct UriQuery {
    uri: Option<String>,
}

/// POST /favorites/add - add a plugin to favorites
#[post("/favorites/add")]
pub async fn favorites_add(
    form: web::Form<UriQuery>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let uri = form.uri.as_deref().unwrap_or("");
    if uri.is_empty() {
        return HttpResponse::BadRequest().json(false);
    }

    // Load, add, save favorites
    let favs_file = &state.settings.favorites_json_file;
    let mut favs: Vec<String> = crate::utils::safe_json_load(favs_file);
    if !favs.contains(&uri.to_string()) {
        favs.push(uri.to_string());
        let json = serde_json::to_string_pretty(&favs).unwrap_or_default();
        let _ = crate::utils::text_file_flusher(favs_file, &json);
    }

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(true)
}

/// POST /favorites/remove - remove a plugin from favorites
#[post("/favorites/remove")]
pub async fn favorites_remove(
    form: web::Form<UriQuery>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let uri = form.uri.as_deref().unwrap_or("");
    if uri.is_empty() {
        return HttpResponse::BadRequest().json(false);
    }

    let favs_file = &state.settings.favorites_json_file;
    let mut favs: Vec<String> = crate::utils::safe_json_load(favs_file);
    favs.retain(|u| u != uri);
    let json = serde_json::to_string_pretty(&favs).unwrap_or_default();
    let _ = crate::utils::text_file_flusher(favs_file, &json);

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(true)
}

#[derive(Deserialize)]
pub struct ConfigQuery {
    key: Option<String>,
    value: Option<String>,
}

/// POST /config/set - set a single preference value
#[post("/config/set")]
pub async fn config_set(
    form: web::Form<ConfigQuery>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let key = form.key.as_deref().unwrap_or("");
    let value = form.value.as_deref().unwrap_or("");

    if key.is_empty() {
        return HttpResponse::BadRequest().json(false);
    }

    let mut session = state.session.write().await;
    session.prefs.set_and_save(key, Value::String(value.to_string()));

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(true)
}

#[derive(Deserialize)]
pub struct UserIdQuery {
    name: Option<String>,
    email: Option<String>,
}

/// POST /save_user_id - save user identity
#[post("/save_user_id")]
pub async fn save_user_id(
    form: web::Form<UserIdQuery>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let name = form.name.as_deref().unwrap_or("");
    let email = form.email.as_deref().unwrap_or("");

    let keys_path = std::path::Path::new(&state.settings.keys_path);
    let user_id_file = keys_path.join("user-id.json");

    let data = json!({"name": name, "email": email});
    let json = serde_json::to_string_pretty(&data).unwrap_or_default();
    let _ = crate::utils::text_file_flusher(&user_id_file, &json);

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(true)
}

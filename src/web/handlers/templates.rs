use actix_web::{get, web, HttpRequest, HttpResponse};
use regex::Regex;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::template;
use crate::utils;
use crate::AppState;

fn get_version(state: &AppState) -> String {
    if let Some(ref v) = state.settings.image_version {
        let v = v.strip_prefix('v').unwrap_or(v);
        urlencoding::encode(v).to_string()
    } else {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        secs.to_string()
    }
}

#[allow(dead_code)]
fn timeless_headers(resp: &mut HttpResponse) {
    resp.headers_mut().insert(
        actix_web::http::header::CACHE_CONTROL,
        "no-store".parse().unwrap(),
    );
}

async fn build_context(
    section: &str,
    version: &str,
    state: &AppState,
    query: &HashMap<String, String>,
) -> template::Context {
    let mut ctx = template::Context::new();
    ctx.insert("version".into(), version.to_string());

    match section {
        "index" => {
            let session = state.session.read().await;

            // Read templates for default icon and settings
            let default_icon_template = std::fs::read_to_string(&state.settings.default_icon_template)
                .map(|s| utils::mod_squeeze(&s))
                .unwrap_or_default();
            let default_settings_template =
                std::fs::read_to_string(&state.settings.default_settings_template)
                    .map(|s| utils::mod_squeeze(&s))
                    .unwrap_or_default();

            ctx.insert("default_icon_template".into(), default_icon_template);
            ctx.insert("default_settings_template".into(), default_settings_template);
            ctx.insert(
                "default_pedalboard".into(),
                utils::mod_squeeze(&state.settings.default_pedalboard.to_string_lossy()),
            );
            ctx.insert("cloud_url".into(), state.settings.cloud_http_address.clone());
            ctx.insert(
                "cloud_labs_url".into(),
                state.settings.cloud_labs_http_address.clone(),
            );
            ctx.insert(
                "plugins_url".into(),
                state.settings.plugins_http_address.clone(),
            );
            ctx.insert(
                "pedalboards_url".into(),
                state.settings.pedalboards_http_address.clone(),
            );
            ctx.insert(
                "pedalboards_labs_url".into(),
                state.settings.pedalboards_labs_http_address.clone(),
            );
            ctx.insert(
                "controlchain_url".into(),
                state.settings.controlchain_http_address.clone(),
            );
            // hardware_profile is base64-encoded JSON of hardware actuators
            {
                use base64::Engine;
                let actuators = "[]"; // No HMI/control chain actuators in this build
                let encoded = base64::engine::general_purpose::STANDARD.encode(actuators.as_bytes());
                ctx.insert("hardware_profile".into(), encoded);
            }

            let hwdesc = utils::get_hardware_descriptor(&state.settings.hardware_desc_file);
            ctx.insert(
                "bin_compat".into(),
                hwdesc.get("bin-compat")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown")
                    .to_string(),
            );
            ctx.insert(
                "codec_truebypass".into(),
                if hwdesc.get("codec_truebypass").and_then(|v| v.as_bool()).unwrap_or(false) {
                    "true".into()
                } else {
                    "false".into()
                },
            );
            ctx.insert(
                "factory_pedalboards".into(),
                if hwdesc.get("factory_pedalboards").and_then(|v| v.as_bool()).unwrap_or(false) {
                    "true".into()
                } else {
                    String::new()
                },
            );
            ctx.insert(
                "platform".into(),
                hwdesc.get("platform")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown")
                    .to_string(),
            );
            ctx.insert(
                "addressing_pages".into(),
                hwdesc.get("addressing_pages")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0)
                    .to_string(),
            );
            ctx.insert(
                "lv2_plugin_dir".into(),
                utils::mod_squeeze(&state.settings.lv2_plugin_dir.to_string_lossy()),
            );

            // Pedalboard state from session
            let pb_name = &session.host.pedalboard.name;
            let pb_path = session.host.pedalboard.path.to_string_lossy().to_string();
            let pb_size = session.host.pedalboard.size;
            let snapshot_name = session.host.snapshot_name().map(|s| s.to_string());

            ctx.insert("bundlepath".into(), utils::mod_squeeze(&pb_path));
            ctx.insert("title".into(), utils::mod_squeeze(pb_name));
            ctx.insert("size".into(), format!("[{}, {}]", pb_size.0, pb_size.1));

            let full_pb_name = if pb_name.is_empty() {
                state.settings.untitled_pedalboard_name.clone()
            } else {
                pb_name.clone()
            };
            let fulltitle = if let Some(ref sn) = snapshot_name {
                format!("{} - {}", full_pb_name, sn)
            } else {
                full_pb_name
            };
            ctx.insert("fulltitle".into(), fulltitle);
            ctx.insert(
                "titleblend".into(),
                if pb_name.is_empty() { "blend".into() } else { String::new() },
            );
            ctx.insert(
                "dev_api_class".into(),
                if state.settings.dev_api {
                    "dev_api".into()
                } else {
                    String::new()
                },
            );
            ctx.insert(
                "using_desktop".into(),
                if state.settings.desktop {
                    "true".into()
                } else {
                    "false".into()
                },
            );
            ctx.insert(
                "using_mod".into(),
                if state.settings.device_key.is_some()
                    && hwdesc.get("platform").and_then(|v| v.as_str()).is_some()
                {
                    "true".into()
                } else {
                    "false".into()
                },
            );

            // User ID
            let user_id: HashMap<String, String> =
                utils::safe_json_load(&state.settings.user_id_json_file);
            ctx.insert(
                "user_name".into(),
                utils::mod_squeeze(user_id.get("name").map(|s| s.as_str()).unwrap_or("")),
            );
            ctx.insert(
                "user_email".into(),
                utils::mod_squeeze(user_id.get("email").map(|s| s.as_str()).unwrap_or("")),
            );

            // Favorites
            let favorites: serde_json::Value =
                utils::safe_json_load_value(&state.settings.favorites_json_file, serde_json::json!([]));
            ctx.insert(
                "favorites".into(),
                serde_json::to_string(&favorites).unwrap_or_else(|_| "[]".to_string()),
            );

            // Preferences
            ctx.insert(
                "preferences".into(),
                serde_json::to_string(&session.prefs.prefs).unwrap_or_else(|_| "{}".to_string()),
            );

            // JACK buffer size and sample rate
            ctx.insert(
                "bufferSize".into(),
                crate::lv2_utils::get_jack_buffer_size().to_string(),
            );
            ctx.insert(
                "sampleRate".into(),
                crate::lv2_utils::get_jack_sample_rate().to_string(),
            );
        }
        "settings" => {
            let session = state.session.read().await;

            ctx.insert("cloud_url".into(), state.settings.cloud_http_address.clone());
            ctx.insert(
                "controlchain_url".into(),
                state.settings.controlchain_http_address.clone(),
            );

            let hwdesc = utils::get_hardware_descriptor(&state.settings.hardware_desc_file);
            ctx.insert(
                "hmi_eeprom".into(),
                if hwdesc.get("hmi_eeprom").and_then(|v| v.as_bool()).unwrap_or(false) {
                    "true".into()
                } else {
                    "false".into()
                },
            );
            ctx.insert(
                "preferences".into(),
                serde_json::to_string(&session.prefs.prefs).unwrap_or_else(|_| "{}".to_string()),
            );
            ctx.insert(
                "bufferSize".into(),
                crate::lv2_utils::get_jack_buffer_size().to_string(),
            );
            ctx.insert(
                "sampleRate".into(),
                crate::lv2_utils::get_jack_sample_rate().to_string(),
            );
        }
        "pedalboard" => {
            let default_icon_template =
                std::fs::read_to_string(&state.settings.default_icon_template)
                    .map(|s| utils::mod_squeeze(&s))
                    .unwrap_or_default();
            let default_settings_template =
                std::fs::read_to_string(&state.settings.default_settings_template)
                    .map(|s| utils::mod_squeeze(&s))
                    .unwrap_or_default();
            ctx.insert("default_icon_template".into(), default_icon_template);
            ctx.insert("default_settings_template".into(), default_settings_template);

            // Get pedalboard info from bundlepath query parameter
            let bundlepath = query.get("bundlepath").map(|s| s.as_str()).unwrap_or("");
            let pedalboard = if !bundlepath.is_empty() {
                crate::lv2_utils::get_pedalboard_info(bundlepath).unwrap_or_else(|| {
                    serde_json::json!({
                        "height": 0, "width": 0, "title": "",
                        "connections": [], "plugins": [], "hardware": {}
                    })
                })
            } else {
                serde_json::json!({
                    "height": 0, "width": 0, "title": "",
                    "connections": [], "plugins": [], "hardware": {}
                })
            };
            use base64::Engine;
            let pb_json = serde_json::to_string(&pedalboard).unwrap_or_default();
            let encoded = base64::engine::general_purpose::STANDARD.encode(pb_json.as_bytes());
            ctx.insert("pedalboard".into(), encoded);
        }
        "allguis" => {
            // version already inserted
        }
        _ => {}
    }

    ctx
}

/// Parse query string into a HashMap
fn parse_query(req: &HttpRequest) -> HashMap<String, String> {
    req.query_string()
        .split('&')
        .filter(|s| !s.is_empty())
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            Some((parts.next()?.to_string(), parts.next().unwrap_or("").to_string()))
        })
        .collect()
}

/// Serve a template-rendered HTML page.
/// Handles version redirect logic from the original TemplateHandler.
async fn serve_template(
    path: &str,
    req: &HttpRequest,
    state: &AppState,
) -> HttpResponse {
    let query = parse_query(req);
    let cur_version = get_version(state);

    // Version redirect logic
    let version = query.get("v").cloned();
    if version.is_none() {
        let uri = req.uri().to_string();
        let sep = if req.query_string().is_empty() { "?" } else { "&" };
        return HttpResponse::Found()
            .insert_header(("Location", format!("{}{}v={}", uri, sep, cur_version)))
            .finish();
    }
    let version = version.unwrap();
    if state.settings.image_version.is_some() && version != cur_version {
        let uri = req.uri().to_string().replace(
            &format!("v={}", version),
            &format!("v={}", cur_version),
        );
        return HttpResponse::Found()
            .insert_header(("Location", uri))
            .finish();
    }

    // Determine actual file
    let file = if path.is_empty() || path == "/" {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };

    let section = file.split('.').next().unwrap_or("index");
    let file_path = state.settings.html_dir.join(file);

    if !file_path.exists() {
        return HttpResponse::Found()
            .insert_header(("Location", format!("/?v={}", cur_version)))
            .finish();
    }

    let ctx = build_context(section, &version, state, &query).await;

    match template::render_file(&file_path, &ctx) {
        Ok(html) => {
            HttpResponse::Ok()
                .content_type("text/html; charset=utf-8")
                .body(html)
        }
        Err(e) => {
            tracing::error!("Template render error for {}: {}", file, e);
            HttpResponse::InternalServerError().body(format!("Template error: {}", e))
        }
    }
}

/// GET /{page}.html - serve template-rendered pages
#[get("/{page}.html")]
pub async fn template_page(
    path: web::Path<String>,
    req: HttpRequest,
    state: web::Data<AppState>,
) -> HttpResponse {
    let page = path.into_inner();
    serve_template(&format!("{}.html", page), &req, &state).await
}

/// GET / - serve index.html
#[get("/")]
pub async fn template_page_bare(req: HttpRequest, state: web::Data<AppState>) -> HttpResponse {
    serve_template("index.html", &req, &state).await
}

/// GET /allguis, /settings, /sdk - redirect aliases
#[get("/{alias:allguis|settings|sdk}")]
pub async fn template_page_alias(
    path: web::Path<String>,
    req: HttpRequest,
    state: web::Data<AppState>,
) -> HttpResponse {
    let alias = path.into_inner();
    if alias == "sdk" {
        // SDK redirects to port 9000
        let full_url = req.uri().to_string().replace("/sdk", ":9000");
        return HttpResponse::MovedPermanently()
            .insert_header(("Location", full_url))
            .finish();
    }
    let version = get_version(&state);
    HttpResponse::MovedPermanently()
        .insert_header(("Location", format!("/{}.html?v={}", alias, version)))
        .finish()
}

/// GET /load_template/{name}.html - load a template include as plain text
#[get("/load_template/{name}.html")]
pub async fn load_template(
    path: web::Path<String>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let name = path.into_inner();
    let file_path = state.settings.html_dir.join("include").join(format!("{}.html", name));
    match std::fs::read_to_string(&file_path) {
        Ok(contents) => HttpResponse::Ok()
            .content_type("text/plain; charset=utf-8")
            .insert_header(("Cache-Control", "no-store"))
            .body(contents),
        Err(_) => HttpResponse::NotFound().finish(),
    }
}

/// GET /js/templates.js - bulk-load all include templates as JS
#[get("/js/templates.js")]
pub async fn bulk_template_loader(state: web::Data<AppState>) -> HttpResponse {
    let include_dir = state.settings.html_dir.join("include");
    let re = Regex::new(r"^[a-z_]+\.html$").unwrap();
    let mut output = String::new();

    if let Ok(entries) = std::fs::read_dir(&include_dir) {
        let mut names: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                if re.is_match(&name) {
                    Some(name)
                } else {
                    None
                }
            })
            .collect();
        names.sort();

        for name in names {
            let path = include_dir.join(&name);
            if let Ok(contents) = std::fs::read_to_string(&path) {
                let key = name.strip_suffix(".html").unwrap_or(&name);
                let squeezed = utils::mod_squeeze(&contents);
                output.push_str(&format!("TEMPLATES['{}'] = '{}';\n\n", key, squeezed));
            }
        }
    }

    HttpResponse::Ok()
        .content_type("text/javascript; charset=utf-8")
        .insert_header(("Cache-Control", "public, max-age=31536000"))
        .insert_header(("Expires", "Mon, 31 Dec 2035 12:00:00 gmt"))
        .body(output)
}

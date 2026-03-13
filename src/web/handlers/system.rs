// System-related endpoints, ported from webserver.py
// /system/info, /system/prefs, /system/exechange, /system/cleanup, /reset

use actix_web::{get, post, web, HttpResponse};
use serde_json::json;

use crate::AppState;

/// GET /system/info - system information
#[get("/system/info")]
pub async fn system_info(state: web::Data<AppState>) -> HttpResponse {
    let utsname = uname_info();
    let devices = get_connected_devices();
    let resp = json!({
        "hwname": "rustyfoot",
        "architecture": std::env::consts::ARCH,
        "cpu": cpu_model(),
        "platform": std::env::consts::OS,
        "model": state.settings.device_key.as_deref().unwrap_or("unknown"),
        "version": state.settings.image_version,
        "uname": utsname,
        "devices": devices,
    });
    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(resp)
}

/// Query JACK hardware ports and group them by device.
fn get_connected_devices() -> Vec<serde_json::Value> {
    use std::collections::BTreeMap;

    // Collect all hardware ports
    let audio_ins = crate::lv2_utils::get_jack_hardware_ports(true, false);
    let audio_outs = crate::lv2_utils::get_jack_hardware_ports(true, true);
    let midi_ins = crate::lv2_utils::get_jack_hardware_ports(false, false);
    let midi_outs = crate::lv2_utils::get_jack_hardware_ports(false, true);

    // Group by device name extracted from port alias
    // Alias format: "alsa_pcm:DeviceName/port_type" or similar
    let mut devices: BTreeMap<String, (usize, usize, usize, usize)> = BTreeMap::new();

    let extract_device = |port: &str| -> String {
        if let Some(alias) = crate::lv2_utils::get_jack_port_alias(port) {
            // alias like "alsa_pcm:USB-Audio/midi_capture_1"
            let name_part = alias.split_once(':').map(|(_, r)| r).unwrap_or(&alias);
            let device = name_part.split_once('/').map(|(d, _)| d).unwrap_or(name_part);
            device.replace('-', " ")
        } else {
            // Fallback: use JACK client name
            port.split_once(':').map(|(c, _)| c).unwrap_or(port).to_string()
        }
    };

    for port in &audio_ins {
        let dev = extract_device(port);
        devices.entry(dev).or_insert((0, 0, 0, 0)).0 += 1;
    }
    for port in &audio_outs {
        let dev = extract_device(port);
        devices.entry(dev).or_insert((0, 0, 0, 0)).1 += 1;
    }
    for port in &midi_ins {
        let dev = extract_device(port);
        devices.entry(dev).or_insert((0, 0, 0, 0)).2 += 1;
    }
    for port in &midi_outs {
        let dev = extract_device(port);
        devices.entry(dev).or_insert((0, 0, 0, 0)).3 += 1;
    }

    devices.into_iter().map(|(name, (ai, ao, mi, mo))| {
        json!({
            "name": name,
            "audio_ins": ai,
            "audio_outs": ao,
            "midi_ins": mi,
            "midi_outs": mo,
        })
    }).collect()
}

fn cpu_model() -> String {
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|contents| {
            contents.lines().find_map(|line| {
                let (key, val) = line.split_once(':')?;
                if key.trim() == "model name" || key.trim() == "Model" {
                    Some(val.trim().to_string())
                } else {
                    None
                }
            })
        })
        .unwrap_or_default()
}

fn uname_info() -> serde_json::Value {
    let mut uts: libc::utsname = unsafe { std::mem::zeroed() };
    if unsafe { libc::uname(&mut uts) } == 0 {
        let to_str = |buf: &[std::ffi::c_char]| {
            unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }
                .to_string_lossy()
                .into_owned()
        };
        json!({
            "sysname": to_str(&uts.sysname),
            "machine": to_str(&uts.machine),
            "release": to_str(&uts.release),
            "version": to_str(&uts.version),
        })
    } else {
        json!({
            "sysname": "",
            "machine": "",
            "release": "",
            "version": "",
        })
    }
}

/// GET /system/prefs - user preferences
#[get("/system/prefs")]
pub async fn system_prefs(state: web::Data<AppState>) -> HttpResponse {
    let session = state.session.read().await;
    let prefs = &session.prefs.prefs;
    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(prefs)
}

use serde::Deserialize;

/// POST /system/exechange - execute system changes (file create/write, services)
#[post("/system/exechange")]
pub async fn system_exechange(
    _state: web::Data<AppState>,
    form: web::Form<ExeChangeForm>,
) -> HttpResponse {
    let etype = form.r#type.as_deref().unwrap_or("");

    match etype {
        "filecreate" => {
            let path = form.path.as_deref().unwrap_or("");
            let create = form.create.as_deref() == Some("1");

            // Whitelist allowed paths
            let allowed = [
                "autorestart-hmi",
                "jack-mono-copy",
                "jack-sync-mode",
                "separate-spdif-outs",
                "using-256-frames",
            ];
            if !allowed.contains(&path) {
                return HttpResponse::Ok()
                    .insert_header(("Cache-Control", "no-store"))
                    .json(false);
            }

            let filename = format!("/data/{}", path);
            if create {
                if let Some(parent) = std::path::Path::new(&filename).parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(&filename, b"");
            } else if std::path::Path::new(&filename).exists() {
                let _ = std::fs::remove_file(&filename);
            }

            crate::utils::os_sync();
            HttpResponse::Ok()
                .insert_header(("Cache-Control", "no-store"))
                .json(true)
        }
        "filewrite" => {
            let path = form.path.as_deref().unwrap_or("");
            let content = form.content.as_deref().unwrap_or("").trim();

            let allowed = ["bluetooth/name"];
            if !allowed.contains(&path) {
                return HttpResponse::Ok()
                    .insert_header(("Cache-Control", "no-store"))
                    .json(false);
            }

            let filename = format!("/data/{}", path);
            if !content.is_empty() {
                if let Some(parent) = std::path::Path::new(&filename).parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(&filename, content);
            } else if std::path::Path::new(&filename).exists() {
                let _ = std::fs::remove_file(&filename);
            }

            crate::utils::os_sync();
            HttpResponse::Ok()
                .insert_header(("Cache-Control", "no-store"))
                .json(true)
        }
        _ => {
            // command, service types are device-specific — not implemented for desktop
            HttpResponse::Ok()
                .insert_header(("Cache-Control", "no-store"))
                .json(json!({"ok": false, "error": "not supported on desktop"}))
        }
    }
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct ExeChangeForm {
    r#type: Option<String>,
    path: Option<String>,
    create: Option<String>,
    content: Option<String>,
    cmd: Option<String>,
    name: Option<String>,
    enable: Option<String>,
    inverted: Option<String>,
    persistent: Option<String>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct CleanupForm {
    banks: Option<String>,
    favorites: Option<String>,
    #[serde(rename = "hmiSettings")]
    hmi_settings: Option<String>,
    #[serde(rename = "licenseKeys")]
    license_keys: Option<String>,
    pedalboards: Option<String>,
    plugins: Option<String>,
}

/// POST /system/cleanup - clean up user data
#[post("/system/cleanup")]
pub async fn system_cleanup(
    state: web::Data<AppState>,
    form: web::Form<CleanupForm>,
) -> HttpResponse {
    let banks = form.banks.as_deref() == Some("1");
    let favorites = form.favorites.as_deref() == Some("1");
    let license_keys = form.license_keys.as_deref() == Some("1");
    let pedalboards = form.pedalboards.as_deref() == Some("1");
    let plugins = form.plugins.as_deref() == Some("1");

    let mut to_delete: Vec<&std::path::Path> = Vec::new();

    if banks {
        to_delete.push(&state.settings.user_banks_json_file);
    }
    if favorites {
        to_delete.push(&state.settings.favorites_json_file);
    }
    if license_keys {
        to_delete.push(std::path::Path::new(&state.settings.keys_path));
    }
    if pedalboards {
        to_delete.push(&state.settings.lv2_pedalboards_dir);
    }
    if plugins {
        to_delete.push(&state.settings.lv2_plugin_dir);
    }

    if to_delete.is_empty() {
        return HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(json!({"ok": false, "error": "Nothing to delete"}));
    }

    for path in &to_delete {
        if path.is_dir() {
            let _ = std::fs::remove_dir_all(path);
        } else if path.is_file() {
            let _ = std::fs::remove_file(path);
        }
    }

    crate::utils::os_sync();

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(json!({"ok": true, "error": ""}))
}

/// GET /reset - reset current pedalboard
#[get("/reset")]
pub async fn reset(state: web::Data<AppState>) -> HttpResponse {
    let mut session = state.session.write().await;
    session.reset();
    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(true)
}

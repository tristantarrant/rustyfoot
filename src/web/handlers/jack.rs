// JACK audio and hardware endpoints, ported from webserver.py
// /jack/get_midi_devices, /jack/set_midi_devices, /set_buffersize,
// /reset_xruns, /truebypass

use actix_web::{get, post, web, HttpResponse};
use serde_json::json;
use std::collections::HashMap;

use crate::lv2_utils;
use crate::AppState;

/// Convert a JACK port alias to a human-readable name.
/// Mirrors Python's midi_port_alias_to_name() using alsa-seq mode.
fn midi_alias_to_name(alias: &str) -> String {
    let name = alias.split_once(':').map(|(_, rest)| rest).unwrap_or(alias);
    name.replace('-', " ")
        .replace(';', ".")
        .replace("/midi_capture_", " MIDI ")
        .replace("/midi_playback_", " MIDI ")
}

/// Get a human-readable title for a MIDI port.
/// Uses the JACK port alias (ALSA device name) when available,
/// falls back to the port name itself.
fn midi_port_title(port: &str) -> String {
    // system_midi: prefix (used by some JACK setups)
    if let Some(rest) = port.strip_prefix("system_midi:") {
        let alias = rest.replace(" (in)", "").replace(" (out)", "");
        return midi_alias_to_name(&alias);
    }
    // PipeWire uses "Midi-Bridge:" prefix
    if let Some(rest) = port.strip_prefix("Midi-Bridge:") {
        let alias = rest.replace(" (playback)", "").replace(" (capture)", "");
        return midi_alias_to_name(&alias);
    }
    // system:midi_* ports — look up the JACK port alias for a meaningful name
    if port.starts_with("system:midi_") || port.starts_with("nooice") {
        if let Some(alias) = lv2_utils::get_jack_port_alias(port) {
            return midi_alias_to_name(&alias);
        }
    }
    // For other ports (e.g. "BLE MIDI 1:out"), use the client name
    if let Some((client, _)) = port.split_once(':') {
        return client.to_string();
    }
    String::new()
}

/// GET /jack/get_midi_devices - list MIDI devices
#[get("/jack/get_midi_devices")]
pub async fn jack_get_midi_devices(state: web::Data<AppState>) -> HttpResponse {
    let mut out_ports: HashMap<String, String> = HashMap::new();
    let mut full_ports: HashMap<String, String> = HashMap::new();

    // MIDI output ports (from JACK's perspective: hardware outputs = inputs to the system)
    let midi_outs = lv2_utils::get_jack_hardware_ports(false, true);
    for port in &midi_outs {
        let alias = midi_port_title(port);
        if alias.is_empty() {
            continue;
        }
        out_ports.insert(alias.clone(), port.clone());
    }

    // MIDI input ports
    let midi_ins = lv2_utils::get_jack_hardware_ports(false, false);
    for port in &midi_ins {
        let alias = midi_port_title(port);
        if alias.is_empty() {
            continue;
        }
        let port_id = if let Some(out_port) = out_ports.get(&alias) {
            format!("{};{}", port, out_port)
        } else {
            port.clone()
        };
        full_ports.insert(port_id, alias);
    }

    let mut dev_list: Vec<String> = Vec::new();
    let mut names: HashMap<String, String> = HashMap::new();

    for (port_id, port_alias) in &full_ports {
        dev_list.push(port_id.clone());
        let suffix = if out_ports.contains_key(port_alias) {
            " (in+out)"
        } else {
            " (in)"
        };
        names.insert(port_id.clone(), format!("{}{}", port_alias, suffix));
    }

    dev_list.sort();

    let session = state.session.read().await;
    let aggregated = session.host.midi_aggregated_mode;

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(json!({
            "devsInUse": dev_list,
            "devList": dev_list,
            "names": names,
            "midiAggregatedMode": aggregated,
        }))
}

/// POST /jack/set_midi_devices - configure MIDI devices
#[post("/jack/set_midi_devices")]
pub async fn jack_set_midi_devices(
    body: String,
    state: web::Data<AppState>,
) -> HttpResponse {
    if let Ok(data) = serde_json::from_str::<serde_json::Value>(&body) {
        let aggregated = data.get("midiAggregatedMode").and_then(|v| v.as_bool()).unwrap_or(true);
        let loopback = data.get("midiLoopback").and_then(|v| v.as_bool()).unwrap_or(false);

        let mut session = state.session.write().await;
        let was_aggregated = session.host.midi_aggregated_mode;

        session.host.midi_aggregated_mode = aggregated;
        session.host.midi_loopback_enabled = loopback;

        // Broadcast port changes if mode changed
        if aggregated != was_aggregated {
            if aggregated {
                // Separated → Aggregated: remove individual ports, add aggregate ports
                let midi_ins = lv2_utils::get_jack_hardware_ports(false, false);
                for port in &midi_ins {
                    if let Some(short) = port.split_once(':').map(|(_, r)| r) {
                        session.msg_callback(&format!("remove_hw_port /graph/{}", short));
                    }
                }
                let midi_outs = lv2_utils::get_jack_hardware_ports(false, true);
                for port in &midi_outs {
                    if let Some(short) = port.split_once(':').map(|(_, r)| r) {
                        session.msg_callback(&format!("remove_hw_port /graph/{}", short));
                    }
                }
                session.msg_callback("add_hw_port /graph/midi_merger_out midi 0 All_MIDI_In 1");
                session.msg_callback("add_hw_port /graph/midi_broadcaster_in midi 1 All_MIDI_Out 1");
            } else {
                // Aggregated → Separated: remove aggregate ports, add individual ports
                session.msg_callback("remove_hw_port /graph/midi_merger_out");
                session.msg_callback("remove_hw_port /graph/midi_broadcaster_in");

                let midi_ins = lv2_utils::get_jack_hardware_ports(false, false);
                for (i, port) in midi_ins.iter().enumerate() {
                    if !port.starts_with("system:midi_") {
                        continue;
                    }
                    let short = port.split_once(':').map(|(_, r)| r).unwrap_or(port);
                    let title = midi_port_title_for_ws(port);
                    session.msg_callback(&format!(
                        "add_hw_port /graph/{} midi 0 {} {}", short, title, i + 1
                    ));
                }
                let midi_outs = lv2_utils::get_jack_hardware_ports(false, true);
                for (i, port) in midi_outs.iter().enumerate() {
                    if !port.starts_with("system:midi_") {
                        continue;
                    }
                    let short = port.split_once(':').map(|(_, r)| r).unwrap_or(port);
                    let title = midi_port_title_for_ws(port);
                    session.msg_callback(&format!(
                        "add_hw_port /graph/{} midi 1 {} {}", short, title, i + 1
                    ));
                }
            }
        }
    }

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(true)
}

/// Get a human-readable title for a MIDI port, formatted for WebSocket messages (underscores for spaces).
fn midi_port_title_for_ws(port: &str) -> String {
    let title = if let Some(alias) = lv2_utils::get_jack_port_alias(port) {
        let name = alias.split_once(':').map(|(_, r)| r).unwrap_or(&alias);
        name.replace('-', " ")
            .replace(';', ".")
            .replace("/midi_capture_", " MIDI ")
            .replace("/midi_playback_", " MIDI ")
    } else {
        port.split_once(':').map(|(_, r)| r).unwrap_or(port).to_string()
    };
    title.replace(' ', "_")
}

/// POST /set_buffersize/{size} - change JACK buffer size
#[post("/set_buffersize/{size}")]
pub async fn set_buffersize(
    path: web::Path<u32>,
    _state: web::Data<AppState>,
) -> HttpResponse {
    let size = path.into_inner();
    if size != 128 && size != 256 {
        return HttpResponse::BadRequest().json(json!({"ok": false}));
    }
    let actual = lv2_utils::set_jack_buffer_size(size);
    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(json!({"ok": true, "size": actual}))
}

/// POST /reset_xruns - reset JACK xrun counter
#[post("/reset_xruns")]
pub async fn reset_xruns(_state: web::Data<AppState>) -> HttpResponse {
    lv2_utils::reset_xruns();
    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(true)
}

/// GET /truebypass/{channel}/{state} - set true bypass
#[get("/truebypass/{channel}/{bypassed}")]
pub async fn truebypass(
    path: web::Path<(String, String)>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let (channel, bypassed) = path.into_inner();
    let right = channel == "Right" || channel == "1";
    let bypass = bypassed == "true" || bypassed == "1";
    let ok = lv2_utils::set_truebypass_value(right, bypass);

    // Notify WebSocket clients
    let session = state.session.read().await;
    let left_val = if !right && bypass { 1 } else { if lv2_utils::get_truebypass_value(false) { 1 } else { 0 } };
    let right_val = if right && bypass { 1 } else { if lv2_utils::get_truebypass_value(true) { 1 } else { 0 } };
    session.msg_callback(&format!("truebypass {} {}", left_val, right_val));

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(ok)
}

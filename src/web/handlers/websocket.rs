// WebSocket endpoint, ported from webserver.py ServerWebSocket handler.
// /websocket - main client connection for real-time communication

use actix_web::{get, web, HttpRequest, HttpResponse};
use actix_ws::Message;
use std::sync::atomic::Ordering;
use tokio::io::AsyncReadExt;

use crate::AppState;
use crate::session::SharedSession;

/// GET /websocket - main WebSocket endpoint
#[get("/websocket")]
pub async fn websocket(
    req: HttpRequest,
    stream: web::Payload,
    state: web::Data<AppState>,
) -> Result<HttpResponse, actix_web::Error> {
    let (response, mut session_ws, mut msg_stream) = actix_ws::handle(&req, stream)?;

    let state = state.into_inner();

    // Subscribe to broadcast messages from the session
    let mut broadcast_rx = state.ws_broadcast.subscribe();

    actix_web::rt::spawn(async move {
        // Connect to mod-host if not already connected
        let read_stream = {
            let mut session = state.session.write().await;
            session.host.start_session().await
        };

        // If we got a new read stream, spawn the mod-host read loop
        if let Some(read_stream) = read_stream {
            if !state.read_loop_running.swap(true, Ordering::SeqCst) {
                let session_for_reader = state.session.clone();
                let flag = state.read_loop_running.clone();
                actix_web::rt::spawn(async move {
                    mod_host_read_loop(read_stream, session_for_reader).await;
                    flag.store(false, Ordering::SeqCst);
                });
            }
        }

        // Send initial state to the client (report_current_state from host.py)
        report_current_state(&mut session_ws, &state).await;

        // Periodic stats timer (CPU load, xruns) — mirrors host.py statstimer_callback
        let mut stats_interval = tokio::time::interval(std::time::Duration::from_secs(1));
        stats_interval.tick().await; // consume the immediate first tick

        loop {
            tokio::select! {
                // Messages from the WebSocket client
                ws_msg = msg_stream.recv() => {
                    match ws_msg {
                        Some(Ok(Message::Text(text))) => {
                            handle_ws_message(&text, &state, &mut session_ws).await;
                        }
                        Some(Ok(Message::Ping(bytes))) => {
                            let _ = session_ws.pong(&bytes).await;
                        }
                        Some(Ok(Message::Close(_))) | None => {
                            break;
                        }
                        _ => {}
                    }
                }
                // Broadcast messages from the session (e.g., plugin added/removed)
                broadcast_msg = broadcast_rx.recv() => {
                    match broadcast_msg {
                        Ok(msg) => {
                            if session_ws.text(msg).await.is_err() {
                                break;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!("[websocket] broadcast lagged by {} messages", n);
                        }
                        Err(_) => {
                            break;
                        }
                    }
                }
                // Periodic stats update (CPU load, xruns)
                _ = stats_interval.tick() => {
                    let msg = get_stats_message();
                    if session_ws.text(msg).await.is_err() {
                        break;
                    }
                }
            }
        }

        tracing::debug!("[websocket] connection closed");
    });

    Ok(response)
}

/// Send the current state to a newly connected WebSocket client.
/// This mirrors host.py's report_current_state().
async fn report_current_state(ws: &mut actix_ws::Session, state: &AppState) {
    let session = state.session.read().await;

    // System stats (memory, cpu freq, cpu temp)
    let _ = ws.text(session.host.get_system_stats_message()).await;

    // Stats (CPU load, xruns)
    let _ = ws.text(get_stats_message()).await;

    // Transport state
    let transport = &session.host.transport;
    let _ = ws
        .text(format!(
            "transport {} {} {} {}",
            if transport.rolling { 1 } else { 0 },
            transport.bpb,
            transport.bpm,
            transport.sync.as_str(),
        ))
        .await;

    // True bypass state
    let left = if crate::lv2_utils::get_truebypass_value(false) { 1 } else { 0 };
    let right = if crate::lv2_utils::get_truebypass_value(true) { 1 } else { 0 };
    let _ = ws.text(format!("truebypass {} {}", left, right)).await;

    // Loading start: empty=1, modified=0 (no pedalboard loaded yet)
    let pedalboard = &session.host.pedalboard;
    let _ = ws
        .text(format!(
            "loading_start {} {}",
            if pedalboard.empty { 1 } else { 0 },
            if pedalboard.modified { 1 } else { 0 },
        ))
        .await;

    // Pedalboard size
    let _ = ws
        .text(format!(
            "size {} {}",
            pedalboard.size.0, pedalboard.size.1,
        ))
        .await;

    // Hardware audio ports (dynamically enumerated from JACK)
    let audio_ins = crate::lv2_utils::get_jack_hardware_ports(true, false);
    for (i, port) in audio_ins.iter().enumerate() {
        let port_name = port.split_once(':').map(|(_, r)| r).unwrap_or(port);
        let title = port_name.replace(|c: char| !c.is_alphanumeric() && c != '_', "_");
        let title = title[..1].to_uppercase() + &title[1..];
        let _ = ws
            .text(format!("add_hw_port /graph/{} audio 0 {} {}", port_name, title, i + 1))
            .await;
    }
    let audio_outs = crate::lv2_utils::get_jack_hardware_ports(true, true);
    for (i, port) in audio_outs.iter().enumerate() {
        let port_name = port.split_once(':').map(|(_, r)| r).unwrap_or(port);
        let title = port_name.replace(|c: char| !c.is_alphanumeric() && c != '_', "_");
        let title = title[..1].to_uppercase() + &title[1..];
        let _ = ws
            .text(format!("add_hw_port /graph/{} audio 1 {} {}", port_name, title, i + 1))
            .await;
    }

    // MIDI ports
    if session.host.midi_aggregated_mode {
        let _ = ws
            .text("add_hw_port /graph/midi_merger_out midi 0 All_MIDI_In 1")
            .await;
        let _ = ws
            .text("add_hw_port /graph/midi_broadcaster_in midi 1 All_MIDI_Out 1")
            .await;
    } else {
        // Separated mode: enumerate individual MIDI hardware ports
        let midi_ins = crate::lv2_utils::get_jack_hardware_ports(false, false);
        for (i, port) in midi_ins.iter().enumerate() {
            if !port.starts_with("system:midi_") {
                continue;
            }
            let port_short = port.split_once(':').map(|(_, r)| r).unwrap_or(port);
            let title = midi_port_title_for_hw(port);
            let _ = ws
                .text(format!(
                    "add_hw_port /graph/{} midi 0 {} {}",
                    port_short, title, i + 1
                ))
                .await;
        }
        let midi_outs = crate::lv2_utils::get_jack_hardware_ports(false, true);
        for (i, port) in midi_outs.iter().enumerate() {
            if !port.starts_with("system:midi_") {
                continue;
            }
            let port_short = port.split_once(':').map(|(_, r)| r).unwrap_or(port);
            let title = midi_port_title_for_hw(port);
            let _ = ws
                .text(format!(
                    "add_hw_port /graph/{} midi 1 {} {}",
                    port_short, title, i + 1
                ))
                .await;
        }
    }

    // Send loaded plugins
    use crate::settings::PEDALBOARD_INSTANCE_ID;
    for (&instance_id, plugin_data) in &session.host.plugins {
        if instance_id == PEDALBOARD_INSTANCE_ID {
            continue;
        }
        let has_build_env = if plugin_data.build_env.is_empty() { 0 } else { 1 };
        let _ = ws
            .text(format!(
                "add {} {} {:.1} {:.1} {} {} {}",
                plugin_data.instance,
                plugin_data.uri,
                plugin_data.x as f64,
                plugin_data.y as f64,
                if plugin_data.bypassed { 1 } else { 0 },
                plugin_data.sversion,
                has_build_env,
            ))
            .await;
    }

    // Send plugin parameters, presets, MIDI mappings
    for (&instance_id, plugin_data) in &session.host.plugins {
        if instance_id == PEDALBOARD_INSTANCE_ID {
            continue;
        }
        // Bypass MIDI CC
        if plugin_data.bypass_cc.0 >= 0 && plugin_data.bypass_cc.1 >= 0 {
            let _ = ws
                .text(format!(
                    "midi_map {} :bypass {} {} 0.0 1.0",
                    plugin_data.instance, plugin_data.bypass_cc.0, plugin_data.bypass_cc.1
                ))
                .await;
        }

        // Preset
        if !plugin_data.preset.is_empty() {
            let _ = ws
                .text(format!("preset {} {}", plugin_data.instance, plugin_data.preset))
                .await;
        }

        // Port values
        for (symbol, value) in &plugin_data.ports {
            let _ = ws
                .text(format!(
                    "param_set {} {} {}",
                    plugin_data.instance, symbol, value
                ))
                .await;
        }

        // MIDI CC mappings for ports
        for (symbol, &(ch, ctrl, min, max)) in &plugin_data.midi_ccs {
            if ch >= 0 && ctrl >= 0 {
                let _ = ws
                    .text(format!(
                        "midi_map {} {} {} {} {} {}",
                        plugin_data.instance, symbol, ch, ctrl, min, max
                    ))
                    .await;
            }
        }

        // Output monitor values
        for (symbol, value) in &plugin_data.outputs {
            if let Some(v) = value {
                let _ = ws
                    .text(format!(
                        "output_set {} {} {}",
                        plugin_data.instance, symbol, v
                    ))
                    .await;
            }
        }
    }

    // Send connections
    for conn in session.host.connections.get_all() {
        let _ = ws
            .text(format!("connect {} {}", conn.port_from, conn.port_to))
            .await;
    }

    // Loading end with current snapshot ID
    let _ = ws
        .text(format!(
            "loading_end {}",
            pedalboard.current_snapshot_id,
        ))
        .await;
}

/// Get a human-readable title for a MIDI hardware port, formatted for WebSocket messages.
/// Uses JACK port alias when available, with spaces replaced by underscores.
fn midi_port_title_for_hw(port: &str) -> String {
    let title = if let Some(alias) = crate::lv2_utils::get_jack_port_alias(port) {
        // Transform alias like "alsa_pcm:USB-Audio/midi_capture_1" → "USB Audio MIDI 1"
        let name = alias.split_once(':').map(|(_, r)| r).unwrap_or(&alias);
        name.replace('-', " ")
            .replace(';', ".")
            .replace("/midi_capture_", " MIDI ")
            .replace("/midi_playback_", " MIDI ")
    } else {
        port.split_once(':')
            .map(|(_, r)| r)
            .unwrap_or(port)
            .to_string()
    };
    title.replace(' ', "_")
}

/// Build the "stats <cpu_load> <xruns>" message from JACK data.
fn get_stats_message() -> String {
    let (cpu_load, xruns) = crate::lv2_utils::get_jack_data(false)
        .map(|d| {
            let cpu = d.get("cpuLoad").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let xr = d.get("xruns").and_then(|v| v.as_u64()).unwrap_or(0);
            (cpu, xr)
        })
        .unwrap_or((0.0, 0));
    format!("stats {:.1} {}", cpu_load, xruns)
}

async fn handle_ws_message(
    text: &str,
    state: &AppState,
    _ws: &mut actix_ws::Session,
) {
    let parts: Vec<&str> = text.splitn(2, ' ').collect();
    let cmd = parts[0];
    let args = if parts.len() > 1 { parts[1] } else { "" };

    match cmd {
        "pong" => {
            // Ping response, ignore
        }
        "data_ready" => {
            // Client acknowledges data readiness, no action needed for now
        }
        "param_set" => {
            // Format: "port value"
            if let Some((port, val_str)) = args.rsplit_once(' ') {
                if let Ok(value) = val_str.parse::<f64>() {
                    let mut session = state.session.write().await;
                    session.ws_parameter_set(port, value, None).await;
                }
            }
        }
        "patch_get" => {
            // Format: "instance uri"
            if let Some((instance, uri)) = args.split_once(' ') {
                let mut session = state.session.write().await;
                session.ws_patch_get(instance, uri).await;
            }
        }
        "patch_set" => {
            // Format: "instance uri vtype value"
            let parts: Vec<&str> = args.splitn(4, ' ').collect();
            if parts.len() == 4 {
                let instance = parts[0];
                let uri = parts[1];
                let vtype = parts[2];
                let value = parts[3];
                let mut session = state.session.write().await;
                session.ws_patch_set(instance, uri, vtype, value, None).await;
            }
        }
        "plugin_pos" => {
            // Format: "instance x y"
            let pos_parts: Vec<&str> = args.splitn(3, ' ').collect();
            if pos_parts.len() == 3 {
                let instance = pos_parts[0];
                if let (Ok(x), Ok(y)) = (pos_parts[1].parse::<i32>(), pos_parts[2].parse::<i32>()) {
                    let mut session = state.session.write().await;
                    session.ws_plugin_position(instance, x, y, None);
                }
            }
        }
        "pb_size" => {
            // Format: "width height"
            if let Some((w_str, h_str)) = args.split_once(' ') {
                if let (Ok(w), Ok(h)) = (w_str.parse::<i32>(), h_str.parse::<i32>()) {
                    let mut session = state.session.write().await;
                    session.ws_pedalboard_size(w, h);
                }
            }
        }
        "link_enable" => {
            let mut session = state.session.write().await;
            session.host.set_link_enabled().await;
        }
        "midi_clock_slave_enable" => {
            let mut session = state.session.write().await;
            session.host.set_midi_clock_slave_enabled().await;
        }
        "set_internal_transport_source" => {
            let mut session = state.session.write().await;
            session.host.set_internal_transport_source().await;
        }
        "transport-bpb" => {
            if let Ok(bpb) = args.parse::<f64>() {
                let mut session = state.session.write().await;
                session.host.transport.bpb = bpb;
            }
        }
        "transport-bpm" => {
            if let Ok(bpm) = args.parse::<f64>() {
                let mut session = state.session.write().await;
                session.host.transport.bpm = bpm;
            }
        }
        "transport-rolling" => {
            let rolling = args == "1" || args == "true";
            let mut session = state.session.write().await;
            session.host.transport.rolling = rolling;
        }
        "show_external_ui" => {
            let instance = args.trim();
            if !instance.is_empty() {
                let mut session = state.session.write().await;
                session.ws_show_external_ui(instance).await;
            }
        }
        _ => {
            tracing::debug!("[websocket] unknown command: {}", cmd);
        }
    }
}

/// Read async notifications from mod-host (port N+1) and process them.
/// Runs as a background task for the lifetime of the connection.
async fn mod_host_read_loop(mut read_stream: tokio::net::TcpStream, session: SharedSession) {
    tracing::info!("[mod-host-reader] read loop started");
    let mut buf = vec![0u8; 4096];
    let mut msg_buf = Vec::new();

    loop {
        match read_stream.read(&mut buf).await {
            Ok(0) => {
                tracing::warn!("[mod-host-reader] read socket closed");
                return;
            }
            Ok(n) => {
                msg_buf.extend_from_slice(&buf[..n]);

                while let Some(pos) = msg_buf.iter().position(|&b| b == 0) {
                    let msg_bytes: Vec<u8> = msg_buf.drain(..=pos).collect();
                    let msg = String::from_utf8_lossy(&msg_bytes[..msg_bytes.len() - 1])
                        .trim()
                        .to_string();
                    if !msg.is_empty() {
                        tracing::debug!("[mod-host-reader] received: {}", msg);
                        handle_mod_host_message(&msg, &session).await;
                    }
                }
            }
            Err(e) => {
                tracing::error!("[mod-host-reader] read error: {}", e);
                return;
            }
        }
    }
}

/// Handle an async notification from mod-host.
async fn handle_mod_host_message(msg: &str, session: &SharedSession) {
    let parts: Vec<&str> = msg.splitn(2, ' ').collect();
    let cmd = parts[0];
    let data = if parts.len() > 1 { parts[1] } else { "" };

    match cmd {
        "midi_mapped" => {
            // Format: instance_id portsymbol channel controller value minimum maximum
            let fields: Vec<&str> = data.splitn(7, ' ').collect();
            if fields.len() == 7 {
                let instance_id: i32 = fields[0].parse().unwrap_or(-1);
                let portsymbol = fields[1];
                let channel: i32 = fields[2].parse().unwrap_or(-1);
                let controller: i32 = fields[3].parse().unwrap_or(-1);
                let value: f64 = fields[4].parse().unwrap_or(0.0);
                let minimum: f64 = fields[5].parse().unwrap_or(0.0);
                let maximum: f64 = fields[6].parse().unwrap_or(1.0);

                let session = session.read().await;
                if let Some(instance) = session.host.mapper.get_instance(instance_id) {
                    session.msg_callback(&format!(
                        "midi_map {} {} {} {} {} {}",
                        instance, portsymbol, channel, controller, minimum, maximum
                    ));
                    session.msg_callback(&format!(
                        "param_set {} {} {}",
                        instance, portsymbol, value
                    ));
                }
            }
        }
        "midi_program_change" => {
            // Format: program
            tracing::debug!("[mod-host-reader] midi_program_change: {}", data);
        }
        "transport" => {
            // Format: rolling bpb bpm speed
            let fields: Vec<&str> = data.splitn(4, ' ').collect();
            if fields.len() >= 3 {
                let rolling = fields[0] == "1";
                let bpb: f64 = fields[1].parse().unwrap_or(4.0);
                let bpm: f64 = fields[2].parse().unwrap_or(120.0);

                let session = session.read().await;
                session.msg_callback(&format!(
                    "transport {} {} {} {}",
                    if rolling { 1 } else { 0 },
                    bpb, bpm,
                    session.host.transport.sync.as_str()
                ));
            }
        }
        "output_set" => {
            // Format: instance_id portsymbol value
            let fields: Vec<&str> = data.splitn(3, ' ').collect();
            if fields.len() == 3 {
                let instance_id: i32 = fields[0].parse().unwrap_or(-1);
                let portsymbol = fields[1];
                let value = fields[2];

                let session = session.read().await;
                if let Some(instance) = session.host.mapper.get_instance(instance_id) {
                    session.msg_callback(&format!(
                        "output_set {} {} {}",
                        instance, portsymbol, value
                    ));
                }
            }
        }
        "param_set" => {
            // Format: instance_id portsymbol value
            let fields: Vec<&str> = data.splitn(3, ' ').collect();
            if fields.len() == 3 {
                let instance_id: i32 = fields[0].parse().unwrap_or(-1);
                let portsymbol = fields[1];
                let value = fields[2];

                let session = session.read().await;
                if let Some(instance) = session.host.mapper.get_instance(instance_id) {
                    session.msg_callback(&format!(
                        "param_set {} {} {}",
                        instance, portsymbol, value
                    ));
                }
            }
        }
        "data_finish" => {
            // Sent after a batch of async notifications.
            // Must send output_data_ready back to mod-host to re-enable feedback.
            let mut session = session.write().await;
            session.host.ipc.send_notmodified("output_data_ready", None, "boolean").await;
        }
        _ => {
            tracing::debug!("[mod-host-reader] unhandled: {} {}", cmd, data);
        }
    }
}

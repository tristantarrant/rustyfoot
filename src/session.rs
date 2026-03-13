// Session management, ported from mod/session.py
// Central state broker between web, host, and HMI.

use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::development::FakeHmi;
use crate::hmi::{Hmi, HmiCallback, TcpHmi};
use crate::host::Host;
use crate::lv2_utils;
use crate::recorder::{Player, Recorder};
use crate::screenshot::ScreenshotGenerator;
use crate::settings::Settings;
use crate::utils;

/// Get all user pedalboard names (for ensuring unique titles).
fn get_all_user_pedalboard_names() -> Vec<String> {
    lv2_utils::get_all_pedalboards(lv2_utils::PEDALBOARD_INFO_USER_ONLY)
        .iter()
        .filter_map(|pb| pb.get("title").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .collect()
}

/// Get a unique name by appending " (N)" if the name already exists.
fn get_unique_name(name: &str, names: &[String]) -> String {
    if !names.iter().any(|n| n == name) {
        return name.to_string();
    }
    let mut candidate = format!("{} (2)", name);
    let mut num = 2u32;
    while names.iter().any(|n| n == &candidate) {
        num += 1;
        // Strip old suffix
        if let Some(pos) = candidate.rfind('(') {
            candidate = format!("{}({})", &candidate[..pos], num);
        } else {
            candidate = format!("{} ({})", name, num);
        }
    }
    candidate
}

/// Save last bank and pedalboard path to disk.
fn save_last_bank_and_pedalboard(settings: &Settings, bank: i32, pedalboard: &str) {
    let data = serde_json::json!({
        "bank": bank - 2,
        "pedalboard": pedalboard,
        "supportsDividers": true,
    });
    let json_str = serde_json::to_string(&data).unwrap_or_default();
    if let Err(e) = utils::text_file_flusher(&settings.last_state_json_file, &json_str) {
        tracing::error!("[session] failed to save last state: {}", e);
    }
}

/// User preferences (JSON-backed key-value store).
pub struct UserPreferences {
    pub prefs: HashMap<String, Value>,
    path: std::path::PathBuf,
}

impl UserPreferences {
    pub fn new(path: &Path) -> Self {
        let prefs: HashMap<String, Value> = utils::safe_json_load(path);
        Self {
            prefs,
            path: path.to_owned(),
        }
    }

    /// Get a preference value, with optional type coercion and allowed-values check.
    pub fn get_with_default(&self, key: &str, default: Value) -> Value {
        self.prefs.get(key).cloned().unwrap_or(default)
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.prefs.get(key)
    }

    /// Set a preference and save atomically (via tmp+rename).
    pub fn set_and_save(&mut self, key: &str, value: Value) {
        self.prefs.insert(key.to_string(), value);
        self.save_atomic();
    }

    /// Set a preference and save non-atomically (simple overwrite).
    pub fn set_and_save_async(&mut self, key: &str, value: Value) {
        self.prefs.insert(key.to_string(), value);
        self.save_simple();
    }

    fn save_atomic(&self) {
        let json = serde_json::to_string_pretty(&self.prefs).unwrap_or_default();
        if let Err(e) = utils::text_file_flusher(&self.path, &json) {
            tracing::error!("Failed to save preferences atomically: {}", e);
        }
    }

    fn save_simple(&self) {
        let json = serde_json::to_string_pretty(&self.prefs).unwrap_or_default();
        if let Err(e) = utils::atomic_write(&self.path, &json) {
            tracing::error!("Failed to save preferences: {}", e);
        }
    }
}

/// WebSocket sender trait — will be implemented by the websocket handler.
pub trait WebSocketSender: Send + Sync {
    fn send_message(&self, msg: &str);
    fn close(&self);
}

/// The main application session.
pub struct Session {
    pub prefs: UserPreferences,
    pub hmi: Box<dyn Hmi>,
    pub host: Host,
    pub recorder: Recorder,
    pub player: Player,
    pub screenshot_generator: ScreenshotGenerator,
    pub websockets: Vec<Box<dyn WebSocketSender>>,
    pub ws_broadcast: Option<tokio::sync::broadcast::Sender<String>>,
    pub screenshot_needed: bool,
}

impl Session {
    pub fn new(settings: &Settings) -> Self {
        let prefs = UserPreferences::new(&settings.preferences_json_file);

        // Initialize HMI
        let hmi: Box<dyn Hmi> = if settings.dev_hmi {
            Box::new(FakeHmi::new())
        } else {
            let tcp_hmi = TcpHmi::new(
                &settings.hmi_tcp_host,
                settings.hmi_tcp_port as u16,
                settings.hmi_timeout as u64,
                &settings.hardware_desc_file,
            );
            Box::new(tcp_hmi)
        };

        let host = Host::new(settings);
        let recorder = Recorder::new(settings);
        let player = Player::new(settings);
        let screenshot_generator = ScreenshotGenerator::new(settings);

        Self {
            prefs,
            hmi,
            host,
            recorder,
            player,
            screenshot_generator,
            websockets: Vec::new(),
            ws_broadcast: None,
            screenshot_needed: false,
        }
    }

    // -------------------------------------------------------------------------
    // WebSocket management

    /// A new browser page connected.
    pub fn websocket_opened(&mut self, ws: Box<dyn WebSocketSender>) {
        let is_first = self.websockets.is_empty();
        self.websockets.push(ws);

        if is_first {
            tracing::info!("[session] first websocket connected, starting UI session");
            // host.start_session() is async, will be called from the handler
        }
    }

    /// A browser page disconnected.
    pub fn websocket_closed(&mut self, ws_index: usize) {
        if ws_index < self.websockets.len() {
            self.websockets.remove(ws_index);
        }

        if self.websockets.is_empty() {
            tracing::info!("[session] last websocket closed, ending UI session");
            // host.end_session() is async, will be called from the handler
        }
    }

    /// Send a message to all connected websockets.
    pub fn msg_callback(&self, msg: &str) {
        // Send via broadcast channel (for actix_ws connections)
        if let Some(ref tx) = self.ws_broadcast {
            let _ = tx.send(msg.to_string());
        }
        // Also send via legacy WebSocketSender trait
        for ws in &self.websockets {
            ws.send_message(msg);
        }
    }

    /// Send a message to all websockets except the sender.
    pub fn msg_callback_broadcast(&self, msg: &str, exclude_index: Option<usize>) {
        for (i, ws) in self.websockets.iter().enumerate() {
            if Some(i) != exclude_index {
                ws.send_message(msg);
            }
        }
    }

    // -------------------------------------------------------------------------
    // HMI helpers

    /// Send a ping to HMI.
    pub fn web_ping(&self, callback: HmiCallback) {
        if self.hmi.initialized() {
            self.hmi.ping(callback);
        } else {
            callback(crate::protocol::RespValue::Bool(false));
        }
        self.msg_callback("ping");
    }

    // -------------------------------------------------------------------------
    // Signal handlers (called from OS signals or internal events)

    pub fn signal_save(&self) {
        // TODO: self.host.hmi_save_current_pedalboard(...)
        tracing::debug!("[session] signal_save");
    }

    pub fn signal_device_updated(&self) {
        self.msg_callback("cc-device-updated");
    }

    pub fn signal_disconnect(&mut self) {
        let sockets = std::mem::take(&mut self.websockets);
        for ws in &sockets {
            ws.send_message("stop");
            ws.close();
        }
    }

    // -------------------------------------------------------------------------
    // Web actions (called from HTTP handlers)

    /// Add a plugin.
    pub async fn web_add(&mut self, instance: &str, uri: &str, x: i32, y: i32) {
        self.screenshot_needed = true;

        // Send "add" command to mod-host
        let instance_id = self.host.mapper.get_id(instance);
        let msg = format!("add {} {}", uri, instance_id);
        self.host.ipc.send_modified(&msg, None, "int").await;

        // Track in host state
        let mut plugin_data = crate::host::plugin::PluginData::new(instance, uri, x, y);
        self.host.pedalboard.modified = true;

        // Get plugin info for build env, version, and default port values
        let info = crate::lv2_utils::get_plugin_info(uri);

        let build_env;
        let sversion;
        if let Some(ref info) = info {
            build_env = info.get("buildEnvironment").and_then(|v| v.as_str()).unwrap_or("").to_string();
            sversion = format!("{}_{}_{}_{}",
                info.get("builder").and_then(|v| v.as_i64()).unwrap_or(0),
                info.get("microVersion").and_then(|v| v.as_i64()).unwrap_or(0),
                info.get("minorVersion").and_then(|v| v.as_i64()).unwrap_or(0),
                info.get("release").and_then(|v| v.as_i64()).unwrap_or(0));
            plugin_data.populate_port_defaults(info);
        } else {
            build_env = String::new();
            sversion = "0_0_0_0".to_string();
        }
        let has_build_env = if build_env.is_empty() { 0 } else { 1 };

        self.host.plugins.insert(instance_id, plugin_data);

        // Notify all WebSocket clients that a plugin was added
        let msg = format!("add {} {} {:.1} {:.1} 0 {} {}",
            instance, uri, x as f64, y as f64, sversion, has_build_env);
        self.msg_callback(&msg);
    }

    /// Remove a plugin.
    pub async fn web_remove(&mut self, instance: &str) {
        self.screenshot_needed = true;

        // Send "remove" command to mod-host
        if let Some(instance_id) = self.host.mapper.get_id_without_creating(instance) {
            let msg = format!("remove {}", instance_id);
            self.host.ipc.send_modified(&msg, None, "int").await;
            self.host.plugins.remove(&instance_id);
            self.host.connections.remove_by_prefix(instance);
            self.host.pedalboard.modified = true;
        }

        // Notify all WebSocket clients
        let msg = format!("remove {}", instance);
        self.msg_callback(&msg);
    }

    /// Connect two ports.
    pub async fn web_connect(&mut self, port_from: &str, port_to: &str) {
        self.screenshot_needed = true;

        // Send "connect" command to mod-host (translate /graph/ to JACK names)
        let jack_from = self.host.fix_host_connection_port(port_from);
        let jack_to = self.host.fix_host_connection_port(port_to);
        let msg = format!("connect {} {}", jack_from, jack_to);
        self.host.ipc.send_modified(&msg, None, "int").await;
        self.host.connections.add(port_from, port_to);
        self.host.pedalboard.modified = true;

        // Notify all WebSocket clients (use original /graph/ names)
        let msg = format!("connect {} {}", port_from, port_to);
        self.msg_callback(&msg);
    }

    /// Disconnect two ports.
    pub async fn web_disconnect(&mut self, port_from: &str, port_to: &str) {
        self.screenshot_needed = true;

        // Send "disconnect" command to mod-host (translate /graph/ to JACK names)
        let jack_from = self.host.fix_host_connection_port(port_from);
        let jack_to = self.host.fix_host_connection_port(port_to);
        let msg = format!("disconnect {} {}", jack_from, jack_to);
        self.host.ipc.send_modified(&msg, None, "int").await;
        self.host.connections.remove(port_from, port_to);
        self.host.pedalboard.modified = true;

        // Notify all WebSocket clients (use original /graph/ names)
        let msg = format!("disconnect {} {}", port_from, port_to);
        self.msg_callback(&msg);
    }

    /// Set a plugin parameter via websocket.
    pub async fn ws_parameter_set(&mut self, port: &str, value: f64, ws_index: Option<usize>) {
        if let Some((instance, portsymbol)) = port.rsplit_once('/') {
            if let Some(instance_id) = self.host.mapper.get_id_without_creating(instance) {
                if portsymbol == ":bypass" {
                    self.host.bypass(instance_id, value >= 0.5).await;
                } else {
                    self.host.param_set(instance_id, portsymbol, value).await;
                }
            }
        }

        // JS expects: param_set <instance> <symbol> <value>
        if let Some((instance, symbol)) = port.rsplit_once('/') {
            let msg = format!("param_set {} {} {}", instance, symbol, value);
            self.msg_callback_broadcast(&msg, ws_index);
        }
    }

    /// Address a parameter to a MIDI controller or HMI actuator.
    pub async fn web_parameter_address(&mut self, port: &str, addressing: &serde_json::Value) {
        let (instance, portsymbol) = match port.rsplit_once('/') {
            Some((i, p)) => (i, p),
            None => return,
        };

        let actuator_uri = addressing.get("uri").and_then(|v| v.as_str()).unwrap_or("");
        let minimum = addressing.get("minimum").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let maximum = addressing.get("maximum").and_then(|v| v.as_f64()).unwrap_or(1.0);

        let instance_id = self.host.mapper.get_id(instance);

        if actuator_uri == "/midi-learn" {
            // Enable MIDI learn for this parameter
            let msg = format!("midi_learn {} {} {} {}",
                instance_id, portsymbol, minimum, maximum);
            self.host.ipc.send_modified(&msg, None, "boolean").await;
        } else if actuator_uri == "/midi-unlearn" {
            // Disable MIDI mapping
            let msg = format!("midi_unmap {} {}", instance_id, portsymbol);
            self.host.ipc.send_modified(&msg, None, "boolean").await;
        } else if actuator_uri.starts_with("/midi-custom_") {
            // Custom MIDI CC assignment: uri = "/midi-custom_CH_CC"
            let cc_str = actuator_uri.strip_prefix("/midi-custom_").unwrap_or("");
            let parts: Vec<&str> = cc_str.split('_').collect();
            if parts.len() == 2 {
                if let (Ok(channel), Ok(controller)) = (parts[0].parse::<i32>(), parts[1].parse::<i32>()) {
                    let msg = format!("midi_map {} {} {} {} {} {}",
                        instance_id, portsymbol, channel, controller, minimum, maximum);
                    self.host.ipc.send_modified(&msg, None, "boolean").await;
                }
            }
        } else if actuator_uri == "null" || actuator_uri.is_empty() {
            // Unaddress
            let msg = format!("midi_unmap {} {}", instance_id, portsymbol);
            self.host.ipc.send_modified(&msg, None, "boolean").await;
        }
        // HMI addressing would go here but we skip it per project constraints
    }

    /// Set plugin position on canvas.
    pub fn ws_plugin_position(
        &mut self,
        instance: &str,
        x: i32,
        y: i32,
        ws_index: Option<usize>,
    ) {
        self.screenshot_needed = true;
        self.host.set_position(instance, x, y);
        let msg = format!("plugin_pos {} {} {}", instance, x, y);
        self.msg_callback_broadcast(&msg, ws_index);
    }

    /// Set pedalboard canvas size.
    pub fn ws_pedalboard_size(&mut self, width: i32, height: i32) {
        self.screenshot_needed = true;
        self.host.set_pedalboard_size(width, height);
    }

    /// Get a plugin patch parameter value.
    pub async fn ws_patch_get(&mut self, instance: &str, uri: &str) {
        if let Some(instance_id) = self.host.mapper.get_id_without_creating(instance) {
            let msg = format!("patch_get {} {}", instance_id, uri);
            self.host.ipc.send_modified(&msg, None, "boolean").await;
        }
    }

    /// Set a plugin patch parameter value.
    pub async fn ws_patch_set(
        &mut self,
        instance: &str,
        uri: &str,
        vtype: &str,
        value: &str,
        ws_index: Option<usize>,
    ) {
        if let Some(instance_id) = self.host.mapper.get_id_without_creating(instance) {
            // Update cached parameter value
            if let Some(plugin_data) = self.host.plugins.get_mut(&instance_id) {
                if let Some(param) = plugin_data.parameters.get_mut(uri) {
                    param.0 = serde_json::Value::String(value.to_string());
                } else {
                    plugin_data.parameters.insert(
                        uri.to_string(),
                        (serde_json::Value::String(value.to_string()), vtype.to_string()),
                    );
                }
            }

            let escaped = value.replace('"', "\\\"");
            let msg = format!("patch_set {} {} \"{}\"", instance_id, uri, escaped);
            self.host.ipc.send_modified(&msg, None, "boolean").await;

            // Broadcast to other WS clients
            let writable = self.host.plugins.get(&instance_id)
                .map(|p| p.parameters.contains_key(uri))
                .unwrap_or(false);
            let broadcast = format!(
                "patch_set {} {} {} {} {}",
                instance,
                if writable { 1 } else { 0 },
                uri,
                vtype,
                value,
            );
            self.msg_callback_broadcast(&broadcast, ws_index);
        }
    }

    /// Show the external UI for a plugin.
    pub async fn ws_show_external_ui(&mut self, instance: &str) {
        if let Some(instance_id) = self.host.mapper.get_id_without_creating(instance) {
            let msg = format!("show_external_ui {}", instance_id);
            self.host.ipc.send_notmodified(&msg, None, "boolean").await;
        }
    }

    /// Reset the session (clear pedalboard).
    pub fn reset(&mut self) {
        tracing::debug!("[session] reset");
        self.screenshot_needed = false;
        // TODO: Full reset chain via host and HMI — async
    }

    // -------------------------------------------------------------------------
    // Pedalboard save/load

    /// Save the current pedalboard.
    pub async fn web_save_pedalboard(
        &mut self,
        title: &str,
        as_new: bool,
        settings: &Settings,
    ) -> (bool, String, Option<String>) {
        let pedalboards_dir = &settings.lv2_pedalboards_dir;

        let (bundlepath, new_title) = if !self.host.pedalboard.path.as_os_str().is_empty()
            && !as_new
            && self.host.pedalboard.path.is_dir()
            && self
                .host
                .pedalboard
                .path
                .starts_with(pedalboards_dir)
        {
            // Save over existing
            (
                self.host
                    .pedalboard
                    .path
                    .to_string_lossy()
                    .to_string(),
                None,
            )
        } else {
            // Save new — ensure unique title
            let all_names = get_all_user_pedalboard_names();
            let final_title = get_unique_name(title, &all_names);
            let titlesym = utils::symbolify(&final_title);
            let titlesym = &titlesym[..titlesym.len().min(16)];

            let mut trypath = pedalboards_dir.join(format!("{}.pedalboard", titlesym));

            if trypath.exists() {
                loop {
                    let n: u32 = rand::random::<u32>() % 99999 + 1;
                    trypath = pedalboards_dir.join(format!("{}-{}.pedalboard", titlesym, n));
                    if !trypath.exists() {
                        break;
                    }
                }
            }

            // Ensure pedalboards dir exists
            if !pedalboards_dir.exists() {
                let _ = std::fs::create_dir_all(pedalboards_dir);
            }

            // Create bundle directory
            if let Err(e) = std::fs::create_dir(&trypath) {
                tracing::error!("[session] failed to create bundle dir: {}", e);
                return (false, String::new(), None);
            }

            self.host.pedalboard.path = trypath.clone();
            let new_title = if final_title != title {
                Some(final_title.clone())
            } else {
                None
            };
            (trypath.to_string_lossy().to_string(), new_title)
        };

        let titlesym = utils::symbolify(title);
        let titlesym = &titlesym[..titlesym.len().min(16)];

        self.host.pedalboard.name = title.to_string();
        self.host.pedalboard.empty = false;
        self.host.pedalboard.modified = false;
        self.host.pedalboard.version += 1;

        self.host
            .save_state_to_ttl(&bundlepath, title, titlesym);

        // Save last state
        save_last_bank_and_pedalboard(settings, 0, &bundlepath);

        // Ask mod-host to save extra state
        self.host
            .ipc
            .send_notmodified(
                &format!("state_save \"{}\"", bundlepath),
                None,
                "boolean",
            )
            .await;

        // Reset pedalboard cache so new pedalboard shows in list
        crate::lv2_utils::reset_get_all_pedalboards_cache(
            crate::lv2_utils::PEDALBOARD_INFO_USER_ONLY,
        );

        utils::os_sync();

        // Schedule screenshot generation
        self.screenshot_generator
            .schedule_screenshot(std::path::Path::new(&bundlepath));
        self.screenshot_needed = false;

        // Notify HMI to reload pedalboard list
        self.hmi.send(crate::mod_protocol::CMD_PEDALBOARD_RELOAD_LIST, None, "boolean");

        (true, bundlepath, new_title)
    }

    /// Load a pedalboard from disk.
    pub async fn web_load_pedalboard(
        &mut self,
        bundlepath: &str,
        is_default: bool,
        settings: &Settings,
    ) -> Option<String> {
        let pb = match crate::lv2_utils::get_pedalboard_info(bundlepath) {
            Some(pb) => pb,
            None => {
                tracing::error!("[session] failed to load pedalboard info from {}", bundlepath);
                return None;
            }
        };

        let title = pb
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let width = pb.get("width").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let height = pb.get("height").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let version = pb.get("version").and_then(|v| v.as_u64()).unwrap_or(0) as i32;

        // Clear current state in mod-host
        self.host
            .ipc
            .send_notmodified("remove -1", None, "int")
            .await;
        self.host.mapper.clear();
        self.host.plugins = crate::host::plugin::init_plugins_data();
        self.host.connections.clear();

        // Notify clients: loading starts
        self.msg_callback(&format!(
            "loading_start {} 0",
            if is_default { 1 } else { 0 }
        ));
        self.msg_callback(&format!("size {} {}", width, height));

        // Transport
        let time_info = pb.get("timeInfo");
        let time_available = time_info
            .and_then(|t| t.get("available"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        if time_available & 0x1 != 0 {
            // BPB available
            if let Some(bpb) = time_info.and_then(|t| t.get("bpb")).and_then(|v| v.as_f64()) {
                self.host.transport.bpb = bpb;
            }
        }
        if time_available & 0x2 != 0 {
            // BPM available
            if let Some(bpm) = time_info.and_then(|t| t.get("bpm")).and_then(|v| v.as_f64()) {
                self.host.transport.bpm = bpm;
            }
        }
        if time_available & 0x4 != 0 {
            // Rolling available
            if let Some(rolling) = time_info.and_then(|t| t.get("rolling")).and_then(|v| v.as_bool())
            {
                self.host.transport.rolling = rolling;
            }
        }

        // Send transport to mod-host
        self.host
            .ipc
            .send_notmodified(
                &format!(
                    "transport {} {} {}",
                    if self.host.transport.rolling { 1 } else { 0 },
                    self.host.transport.bpb,
                    self.host.transport.bpm,
                ),
                None,
                "int",
            )
            .await;

        self.msg_callback(&format!(
            "transport {} {} {} {}",
            if self.host.transport.rolling { 1 } else { 0 },
            self.host.transport.bpb,
            self.host.transport.bpm,
            self.host.transport.sync.as_str(),
        ));

        // Load plugins
        let plugins = pb
            .get("plugins")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        // We need to use msg_callback through the broadcast channel
        let ws_broadcast = self.ws_broadcast.clone();
        let msg_cb = |msg: &str| {
            if let Some(ref tx) = ws_broadcast {
                let _ = tx.send(msg.to_string());
            }
        };

        self.host.load_pb_plugins(&plugins, &msg_cb).await;

        // Load connections
        let connections = pb
            .get("connections")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        self.host
            .load_pb_connections(&connections, &msg_cb)
            .await;

        // Load snapshots
        self.host.load_pb_snapshots(bundlepath);

        // Ask mod-host to load state
        self.host
            .ipc
            .send_notmodified(
                &format!("state_load \"{}\"", bundlepath),
                None,
                "boolean",
            )
            .await;

        // Enable processing
        self.host
            .ipc
            .send_notmodified("feature_enable processing 2", None, "boolean")
            .await;

        // Notify loading end
        self.msg_callback(&format!(
            "loading_end {}",
            self.host.pedalboard.current_snapshot_id
        ));

        // Update pedalboard state
        if is_default {
            self.host.pedalboard.empty = true;
            self.host.pedalboard.modified = false;
            self.host.pedalboard.name.clear();
            self.host.pedalboard.path = std::path::PathBuf::new();
            self.host.pedalboard.size = (0, 0);
            self.host.pedalboard.version = 0;
        } else {
            self.host.pedalboard.empty = false;
            self.host.pedalboard.modified = false;
            self.host.pedalboard.name = title.clone();
            self.host.pedalboard.path = std::path::PathBuf::from(bundlepath);
            self.host.pedalboard.size = (width, height);
            self.host.pedalboard.version = version;

            save_last_bank_and_pedalboard(settings, self.host.bank_id, bundlepath);
        }

        self.screenshot_needed = true;
        Some(title)
    }

    // -------------------------------------------------------------------------
    // Recording

    pub fn web_recording_start(&mut self) {
        self.player.stop();
        self.recorder.start();
    }

    pub fn web_recording_stop(&mut self) -> bool {
        self.recorder.stop()
    }

    pub fn web_recording_delete(&mut self) {
        self.player.stop();
    }

    pub fn web_playing_start(&mut self) -> bool {
        self.host.mute();
        self.player.play()
    }

    pub fn web_playing_stop(&mut self) {
        self.player.stop();
        self.host.unmute();
    }
}

pub type SharedSession = Arc<RwLock<Session>>;

pub fn create_session(settings: &Settings) -> SharedSession {
    Arc::new(RwLock::new(Session::new(settings)))
}

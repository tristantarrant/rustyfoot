// Host module — audio engine interface.
// Ported from mod/host.py (split across submodules for manageability).
//
// Submodules:
//   mapper      — InstanceIdMapper (bidirectional numeric/string ID mapping)
//   plugin      — Plugin data structures (PluginData, PluginMap)
//   ipc         — TCP socket IPC with mod-host (message queue, send/receive)
//   transport   — Transport state (BPM, BPB, sync, rolling)
//   connections — Port connections and JACK port tracking
//   pedalboard  — Pedalboard state and snapshot management

pub mod connections;
pub mod ipc;
pub mod mapper;
#[allow(dead_code)]
pub mod pedalboard;
#[allow(dead_code)]
pub mod plugin;
pub mod transport;

use serde_json::Value;

use crate::addressings::Addressings;
use crate::lv2_utils;
use crate::profile::Profile;
use crate::settings::{Settings, PEDALBOARD_INSTANCE_ID};
use crate::utils;

use self::connections::ConnectionManager;
use self::ipc::{HostCallback, HostIpc};
use self::mapper::InstanceIdMapper;
use self::pedalboard::PedalboardState;
use self::plugin::{PluginData, PluginMap};
use self::transport::TransportState;

type MsgCallback = Box<dyn Fn(&str) + Send + Sync>;

/// Convert an LV2 atom type URI to a single-char type code for patch_set messages.
pub fn atom_type_char(type_uri: &str) -> &str {
    match type_uri.rsplit('#').next().unwrap_or("") {
        "Path" => "p",
        "URI" => "u",
        "String" => "s",
        "Bool" => "b",
        "Int" => "i",
        "Long" => "l",
        "Float" => "f",
        "Double" => "g",
        _ => "s",
    }
}

/// The main Host — interface to the mod-host audio engine.
pub struct Host {
    // IPC
    pub ipc: HostIpc,

    // State
    pub plugins: PluginMap,
    pub mapper: InstanceIdMapper,
    pub addressings: Addressings,
    pub connections: ConnectionManager,
    pub pedalboard: PedalboardState,
    pub transport: TransportState,
    pub profile: Profile,

    // Hardware descriptor
    pub descriptor: serde_json::Map<String, Value>,

    // Flags
    pub web_connected: bool,
    pub web_data_ready_counter: i32,
    pub web_data_ready_ok: bool,
    pub first_pedalboard: bool,
    pub profile_applied: bool,
    pub processing_pending_flag: bool,

    // Hardware features
    pub swapped_audio_channels: bool,
    pub tuner_resolution: i32,

    // User-configurable tuner state
    pub current_tuner_port: i32,
    pub current_tuner_mute: bool,
    pub current_tuner_ref_freq: i32,

    // Bank state
    pub bank_id: i32,
    pub supports_factory_banks: bool,
    pub pedalboard_index_offset: i32,
    pub userbanks_offset: i32,
    pub first_user_bank: i32,

    // MIDI state
    pub midi_aggregated_mode: bool,
    pub midi_loopback_enabled: bool,

    // JACK port prefixes
    pub jack_hw_capture_prefix: String,
    pub jack_external_prefix: String,

    // Callbacks (set by Session)
    pub on_pedalboard_saved: Option<Box<dyn Fn(&str) + Send + Sync>>,
    msg_callback: Option<MsgCallback>,
}

impl Host {
    pub fn new(settings: &Settings) -> Self {
        let descriptor = utils::get_hardware_descriptor(&settings.hardware_desc_file);
        let mut addressings = Addressings::new();
        addressings.init(&descriptor);

        let profile = Profile::new(settings);

        let swapped = descriptor
            .get("swapped_audio_channels")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let tuner_res = descriptor
            .get("tuner_resolution")
            .and_then(|v| v.as_i64())
            .unwrap_or(1) as i32;

        let has_factory = descriptor
            .get("factory_pedalboards")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let (supports_factory_banks, pb_idx_offset, userbanks_offset, first_user_bank) =
            if has_factory {
                (true, 0, 2, 1)
            } else {
                (false, 1, 1, 0)
            };

        let has_noisegate = descriptor
            .get("has_noisegate")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let jack_hw_capture_prefix = if has_noisegate {
            "mod-host:out".to_string()
        } else {
            "system:capture_".to_string()
        };

        Self {
            ipc: HostIpc::new("localhost", settings.device_host_port as u16),

            plugins: plugin::init_plugins_data(),
            mapper: InstanceIdMapper::new(),
            addressings,
            connections: ConnectionManager::new(),
            pedalboard: PedalboardState::new(),
            transport: TransportState::default(),
            profile,

            descriptor,

            web_connected: false,
            web_data_ready_counter: 0,
            web_data_ready_ok: true,
            first_pedalboard: true,
            profile_applied: false,
            processing_pending_flag: false,

            swapped_audio_channels: swapped,
            tuner_resolution: tuner_res,

            current_tuner_port: 1,
            current_tuner_mute: false,
            current_tuner_ref_freq: 440,

            bank_id: first_user_bank,
            supports_factory_banks,
            pedalboard_index_offset: pb_idx_offset,
            userbanks_offset,
            first_user_bank,

            midi_aggregated_mode: true,
            midi_loopback_enabled: false,

            jack_hw_capture_prefix,
            jack_external_prefix: "mod-external".to_string(),

            on_pedalboard_saved: None,
            msg_callback: None,
        }
    }

    /// Set the message callback (broadcasts to websockets via Session).
    pub fn set_msg_callback(&mut self, cb: MsgCallback) {
        self.msg_callback = Some(cb);
    }

    /// Send a message to all connected websockets.
    fn msg_callback(&self, msg: &str) {
        if let Some(ref cb) = self.msg_callback {
            cb(msg);
        }
    }

    // -------------------------------------------------------------------------
    // Connection management

    /// Open the IPC connection to mod-host if not already connected.
    /// Returns the read stream for spawning the read loop.
    pub async fn open_connection_if_needed(&mut self) -> Option<tokio::net::TcpStream> {
        if self.ipc.is_connected() {
            return None;
        }

        match self.ipc.connect().await {
            Ok(read_stream) => {
                tracing::info!("[host] connected to mod-host");
                Some(read_stream)
            }
            Err(e) => {
                tracing::error!("[host] failed to connect to mod-host: {}", e);
                None
            }
        }
    }

    // -------------------------------------------------------------------------
    // Plugin management

    /// Add a plugin instance.
    pub async fn add_plugin(
        &mut self,
        instance: &str,
        uri: &str,
        x: i32,
        y: i32,
        callback: HostCallback,
    ) {
        let instance_id = self.mapper.get_id(instance);
        let msg = format!("add {} {} {} {} {}", uri, instance_id, x, y, instance);

        let plugins = &mut self.plugins;
        let plugin_data = PluginData::new(instance, uri, x, y);
        plugins.insert(instance_id, plugin_data);

        self.ipc
            .send_modified(&msg, Some(callback), "int")
            .await;
        self.pedalboard.modified = true;
    }

    /// Remove a plugin instance.
    pub async fn remove_plugin(&mut self, instance: &str, callback: HostCallback) {
        if let Some(instance_id) = self.mapper.get_id_without_creating(instance) {
            // Remove from addressings
            self.addressings.remove_instance(instance_id);

            // Remove connections involving this instance
            self.connections.remove_by_prefix(instance);

            // Remove plugin data
            self.plugins.remove(&instance_id);

            let msg = format!("remove {}", instance_id);
            self.ipc
                .send_modified(&msg, Some(callback), "int")
                .await;
            self.pedalboard.modified = true;
        } else {
            callback(crate::protocol::RespValue::Int(-1));
        }
    }

    // -------------------------------------------------------------------------
    // Connections

    /// Connect two ports.
    pub async fn connect(&mut self, port_from: &str, port_to: &str, callback: HostCallback) {
        self.connections.add(port_from, port_to);
        let jack_from = self.fix_host_connection_port(port_from);
        let jack_to = self.fix_host_connection_port(port_to);
        let msg = format!("connect {} {}", jack_from, jack_to);
        self.ipc
            .send_modified(&msg, Some(callback), "int")
            .await;
        self.pedalboard.modified = true;
    }

    /// Disconnect two ports.
    pub async fn disconnect(&mut self, port_from: &str, port_to: &str, callback: HostCallback) {
        self.connections.remove(port_from, port_to);
        let jack_from = self.fix_host_connection_port(port_from);
        let jack_to = self.fix_host_connection_port(port_to);
        let msg = format!("disconnect {} {}", jack_from, jack_to);
        self.ipc
            .send_modified(&msg, Some(callback), "int")
            .await;
        self.pedalboard.modified = true;
    }

    /// Map URL-style port names (/graph/...) to JACK port names.
    /// e.g. "/graph/capture_1" → "system:capture_1"
    ///      "/graph/playback_1" → "mod-monitor:in_1"
    ///      "/graph/ratatouille/in0" → "effect_N:in0"
    pub fn fix_host_connection_port(&self, port: &str) -> String {
        let parts: Vec<&str> = port.split('/').collect();
        // e.g. "/graph/capture_1" → ["", "graph", "capture_1"]
        // e.g. "/graph/ratatouille/in0" → ["", "graph", "ratatouille", "in0"]

        if parts.len() == 3 {
            let name = parts[2];

            // MIDI special cases
            if name == "serial_midi_in" { return "ttymidi:MIDI_in".to_string(); }
            if name == "serial_midi_out" { return "ttymidi:MIDI_out".to_string(); }
            if name == "midi_merger_out" { return "mod-midi-merger:out".to_string(); }
            if name == "midi_broadcaster_in" { return "mod-midi-broadcaster:in".to_string(); }

            // Playback ports → mod-monitor
            if let Some(num) = name.strip_prefix("playback_") {
                return format!("mod-monitor:in_{}", num);
            }

            // Fake input
            if let Some(num) = name.strip_prefix("fake_capture_") {
                return format!("fake-input:fake_capture_{}", num);
            }

            // CV ports
            if let Some(num) = name.strip_prefix("cv_capture_") {
                return format!("mod-spi2jack:capture_{}", num);
            }
            if let Some(num) = name.strip_prefix("cv_playback_") {
                return format!("mod-jack2spi:playback_{}", num);
            }
            if name == "cv_exp_pedal" { return "mod-spi2jack:exp_pedal".to_string(); }

            // Capture ports → system:capture_N
            if name.starts_with("capture_") {
                return format!("system:{}", name);
            }

            // Default: system:name
            return format!("system:{}", name);
        }

        if parts.len() >= 4 {
            // Plugin port: /graph/InstanceName/PortSymbol
            let instance = format!("/graph/{}", parts[2]);
            let portsymbol = parts[3];
            let instance_id = self.mapper.get_id_without_creating(&instance);
            if let Some(id) = instance_id {
                return format!("effect_{}:{}", id, portsymbol);
            }
        }

        // Fallback: return as-is (shouldn't happen)
        port.to_string()
    }

    // -------------------------------------------------------------------------
    // Parameters

    /// Set a plugin parameter value.
    pub async fn param_set(&mut self, instance_id: i32, symbol: &str, value: f64) {
        if let Some(plugin) = self.plugins.get_mut(&instance_id) {
            plugin.ports.insert(symbol.to_string(), value);
        }

        let msg = format!("param_set {} {} {}", instance_id, symbol, value);
        self.ipc.send_modified(&msg, None, "int").await;
        self.pedalboard.modified = true;
    }

    /// Set bypass state for a plugin.
    pub async fn bypass(&mut self, instance_id: i32, bypassed: bool) {
        if let Some(plugin) = self.plugins.get_mut(&instance_id) {
            plugin.bypassed = bypassed;
        }

        let value = if bypassed { 1 } else { 0 };
        let msg = format!("bypass {} {}", instance_id, value);
        self.ipc.send_modified(&msg, None, "int").await;
        self.pedalboard.modified = true;
    }

    /// Set plugin position on the canvas.
    pub fn set_position(&mut self, instance: &str, x: i32, y: i32) {
        if let Some(instance_id) = self.mapper.get_id_without_creating(instance) {
            if let Some(plugin) = self.plugins.get_mut(&instance_id) {
                plugin.x = x;
                plugin.y = y;
                self.pedalboard.modified = true;
            }
        }
    }

    /// Set pedalboard canvas size.
    pub fn set_pedalboard_size(&mut self, width: i32, height: i32) {
        self.pedalboard.set_size(width, height);
    }

    // -------------------------------------------------------------------------
    // Transport

    /// Set transport BPM.
    pub async fn set_transport_bpm(&mut self, bpm: f64) {
        self.transport.bpm = bpm;
        let msg = format!("transport {}", self.transport.as_command_args());
        self.ipc.send_notmodified(&msg, None, "int").await;
    }

    /// Set transport beats per bar.
    pub async fn set_transport_bpb(&mut self, bpb: f64) {
        self.transport.bpb = bpb;
        let msg = format!("transport {}", self.transport.as_command_args());
        self.ipc.send_notmodified(&msg, None, "int").await;
    }

    /// Set transport rolling state.
    pub async fn set_transport_rolling(&mut self, rolling: bool) {
        self.transport.rolling = rolling;
        let msg = format!("transport {}", self.transport.as_command_args());
        self.ipc.send_notmodified(&msg, None, "int").await;
    }

    // -------------------------------------------------------------------------
    // Pedalboard

    /// Reset the current pedalboard (clear everything).
    pub async fn reset(&mut self, callback: HostCallback) {
        self.mapper.clear();
        self.plugins = plugin::init_plugins_data();
        self.connections.clear();
        self.addressings.clear();
        self.pedalboard.reset();

        self.ipc
            .send_notmodified("remove -1", Some(callback), "int")
            .await;
    }

    /// Get the current snapshot name.
    pub fn snapshot_name(&self) -> Option<&str> {
        self.pedalboard.snapshot_name()
    }

    /// Build a snapshot from the current plugin state and store it.
    pub fn snapshot_make(&mut self, name: &str) -> i32 {
        let mut plugin_ports = std::collections::HashMap::new();
        for (&instance_id, plugin_data) in &self.plugins {
            if instance_id == PEDALBOARD_INSTANCE_ID {
                continue;
            }
            plugin_ports.insert(instance_id, plugin_data.ports.clone());
        }
        self.pedalboard.snapshot_make(name, &plugin_ports)
    }

    /// Save the current snapshot (overwrite in place).
    pub fn snapshot_save(&mut self) -> bool {
        let idx = self.pedalboard.current_snapshot_id;
        if idx < 0 || (idx as usize) >= self.pedalboard.snapshots.len() {
            return false;
        }
        if self.pedalboard.snapshots[idx as usize].is_none() {
            return false;
        }
        let name = self.pedalboard.snapshots[idx as usize]
            .as_ref()
            .unwrap()
            .name
            .clone();
        self.snapshot_make(&name);
        // Put it back at the same index
        let new_snapshot = self.pedalboard.snapshots.pop().and_then(|s| s);
        if let Some(snap) = new_snapshot {
            self.pedalboard.snapshots[idx as usize] = Some(snap);
        }
        true
    }

    /// Save as a new snapshot, returns (new_id, title).
    pub fn snapshot_saveas(&mut self, name: &str) -> (i32, String) {
        let unique_name = self.snapshot_unique_name(name);
        let id = self.snapshot_make(&unique_name);
        self.pedalboard.current_snapshot_id = id;
        (id, unique_name)
    }

    /// Load a snapshot by index — restore all parameter values.
    pub async fn snapshot_load(
        &mut self,
        idx: i32,
        msg_callback: &dyn Fn(&str),
    ) -> bool {
        if idx < 0 || (idx as usize) >= self.pedalboard.snapshots.len() {
            return false;
        }
        let snapshot = match &self.pedalboard.snapshots[idx as usize] {
            Some(s) => s.clone(),
            None => return false,
        };

        self.pedalboard.current_snapshot_id = idx;

        for (instance_name, port_values) in &snapshot.data {
            // instance_name is instance_id in our implementation
            let instance_id = *instance_name;
            let plugin_data = match self.plugins.get_mut(&instance_id) {
                Some(p) => p,
                None => continue,
            };
            let instance = plugin_data.instance.clone();

            for (symbol, &value) in port_values {
                let old_value = plugin_data.ports.get(symbol).copied();
                if old_value == Some(value) {
                    continue;
                }
                plugin_data.ports.insert(symbol.clone(), value);

                self.ipc
                    .send_notmodified(
                        &format!("param_set {} {} {}", instance_id, symbol, value),
                        None,
                        "int",
                    )
                    .await;
                msg_callback(&format!("param_set {} {} {}", instance, symbol, value));
            }
        }

        let name = snapshot.name.clone();
        msg_callback(&format!("pedal_snapshot {} {}", idx, name));
        true
    }

    fn snapshot_unique_name(&self, name: &str) -> String {
        let existing: Vec<String> = self
            .pedalboard
            .snapshots
            .iter()
            .filter_map(|s| s.as_ref().map(|s| s.name.clone()))
            .collect();
        if !existing.iter().any(|n| n == name) {
            return name.to_string();
        }
        let mut num = 2u32;
        loop {
            let candidate = format!("{} ({})", name, num);
            if !existing.iter().any(|n| n == &candidate) {
                return candidate;
            }
            num += 1;
        }
    }

    // -------------------------------------------------------------------------
    // Plugin presets

    /// Load a preset for a plugin instance.
    pub async fn preset_load(
        &mut self,
        instance: &str,
        uri: &str,
        msg_callback: &dyn Fn(&str),
    ) -> bool {
        let instance_id = match self.mapper.get_id_without_creating(instance) {
            Some(id) => id,
            None => return false,
        };

        let plugin_data = match self.plugins.get_mut(&instance_id) {
            Some(p) => p,
            None => return false,
        };

        plugin_data.next_preset = uri.to_string();

        // Send preset_load to mod-host
        let ok = self
            .ipc
            .send_and_wait_bool(&format!("preset_load {} {}", instance_id, uri))
            .await;

        if !ok {
            return false;
        }

        // Get the preset state from mod-host
        let state = self
            .ipc
            .send_and_wait_str(&format!("preset_show {}", uri))
            .await;

        let state = match state {
            Some(s) if !s.is_empty() => s,
            _ => return false,
        };

        // Check that preset wasn't changed during async operations
        let plugin_data = match self.plugins.get_mut(&instance_id) {
            Some(p) => p,
            None => return false,
        };
        if plugin_data.next_preset != uri {
            return false;
        }

        plugin_data.preset = uri.to_string();
        msg_callback(&format!("preset {} {}", instance, uri));

        // Apply port values from the preset state
        let port_values = crate::lv2_utils::get_state_port_values(&state);
        for (symbol, value) in &port_values {
            let current = plugin_data.ports.get(symbol).copied();
            if current == Some(*value) || current.is_none() {
                continue;
            }

            // Clamp to valid range
            let value = if let Some(&(min, max)) = plugin_data.ranges.get(symbol) {
                value.clamp(min, max)
            } else {
                *value
            };

            plugin_data.ports.insert(symbol.clone(), value);
            msg_callback(&format!("param_set {} {} {}", instance, symbol, value));
        }

        true
    }

    /// Save a new preset for a plugin instance.
    pub async fn preset_save_new(
        &mut self,
        instance: &str,
        name: &str,
        settings: &Settings,
    ) -> Option<(String, String)> {
        let instance_id = match self.mapper.get_id_without_creating(instance) {
            Some(id) => id,
            None => return None,
        };

        let plugin_data = match self.plugins.get_mut(&instance_id) {
            Some(p) => p,
            None => return None,
        };

        let plugin_uri = plugin_data.uri.clone();
        let symbolname: String = crate::utils::symbolify(name).chars().take(32).collect();
        let instance_short = instance.replace("/graph/", "");

        let mut presetbundle = format!(
            "{}/{}-{}.lv2",
            settings.lv2_plugin_dir.to_string_lossy(),
            instance_short,
            symbolname
        );

        // If bundle already exists, generate a unique path
        while std::path::Path::new(&presetbundle).exists() {
            presetbundle = format!(
                "{}/{}-{}-{}.lv2",
                settings.lv2_plugin_dir.to_string_lossy(),
                instance_short,
                symbolname,
                rand::random::<u32>() % 100000
            );
        }

        // Send preset_save to mod-host
        let ok = self
            .ipc
            .send_and_wait_bool(&format!(
                "preset_save {} \"{}\" \"{}\" {}.ttl",
                instance_id,
                name.replace('"', "\\\""),
                presetbundle.replace('"', "\\\""),
                symbolname
            ))
            .await;

        if !ok {
            return None;
        }

        crate::lv2_utils::rescan_plugin_presets(&plugin_uri);

        // Add bundle to lilv world
        if !crate::lv2_utils::is_bundle_loaded(&presetbundle) {
            self.ipc
                .send_and_wait_bool(&format!("bundle_add \"{}\"", presetbundle.replace('"', "\\\"")))
                .await;
            crate::lv2_utils::add_bundle_to_lilv_world(&presetbundle);
        }

        let encoded_path = presetbundle.replace('%', "%25").replace('#', "%23");
        let preseturi = format!(
            "file://{}/{}.ttl",
            encoded_path, symbolname
        );

        if let Some(p) = self.plugins.get_mut(&instance_id) {
            p.preset = preseturi.clone();
        }

        crate::utils::os_sync();
        Some((presetbundle, preseturi))
    }

    /// Replace an existing preset for a plugin instance.
    pub async fn preset_save_replace(
        &mut self,
        instance: &str,
        old_uri: &str,
        presetbundle: &str,
        name: &str,
    ) -> Option<(String, String)> {
        let instance_id = match self.mapper.get_id_without_creating(instance) {
            Some(id) => id,
            None => return None,
        };

        let plugin_data = match self.plugins.get(&instance_id) {
            Some(p) => p,
            None => return None,
        };

        if plugin_data.preset != old_uri || !std::path::Path::new(presetbundle).exists() {
            return None;
        }

        let plugin_uri = plugin_data.uri.clone();
        let symbolname: String = crate::utils::symbolify(name).chars().take(32).collect();

        // Remove old preset TTL files (keep bundle dir)
        if old_uri.starts_with("file:///") {
            let oldpath = urlencoding::decode(&old_uri[7..]).unwrap_or_default().to_string();
            if std::path::Path::new(&oldpath).exists() {
                let _ = std::fs::remove_file(&oldpath);
                let manifest = std::path::Path::new(&oldpath)
                    .parent()
                    .map(|p| p.join("manifest.ttl"));
                if let Some(m) = manifest {
                    if m.exists() {
                        let _ = std::fs::remove_file(&m);
                    }
                }
            }
        }

        // Remove bundle from lilv world, then re-add after saving
        if crate::lv2_utils::is_bundle_loaded(presetbundle) {
            self.ipc
                .send_and_wait_bool(&format!(
                    "bundle_remove \"{}\" \"{}\"",
                    presetbundle.replace('"', "\\\""),
                    old_uri.replace('"', "\\\"")
                ))
                .await;
            crate::lv2_utils::remove_bundle_from_lilv_world(presetbundle, Some(old_uri));
        }

        crate::lv2_utils::rescan_plugin_presets(&plugin_uri);
        if let Some(p) = self.plugins.get_mut(&instance_id) {
            p.preset = String::new();
        }

        // Save new preset
        let ok = self
            .ipc
            .send_and_wait_bool(&format!(
                "preset_save {} \"{}\" \"{}\" {}.ttl",
                instance_id,
                name.replace('"', "\\\""),
                presetbundle.replace('"', "\\\""),
                symbolname
            ))
            .await;

        if !ok {
            let _ = std::fs::remove_dir_all(presetbundle);
            crate::utils::os_sync();
            return None;
        }

        // Re-add bundle to lilv world
        self.ipc
            .send_and_wait_bool(&format!("bundle_add \"{}\"", presetbundle.replace('"', "\\\"")))
            .await;
        crate::lv2_utils::add_bundle_to_lilv_world(presetbundle);

        let encoded_path = presetbundle.replace('%', "%25").replace('#', "%23");
        let preseturi = format!("file://{}/{}.ttl", encoded_path, symbolname);

        if let Some(p) = self.plugins.get_mut(&instance_id) {
            p.preset = preseturi.clone();
        }

        crate::utils::os_sync();
        Some((presetbundle.to_string(), preseturi))
    }

    /// Delete a preset for a plugin instance.
    pub async fn preset_delete(
        &mut self,
        instance: &str,
        uri: &str,
        bundlepath: &str,
        msg_callback: &dyn Fn(&str),
    ) -> bool {
        let instance_id = match self.mapper.get_id_without_creating(instance) {
            Some(id) => id,
            None => return false,
        };

        let plugin_data = match self.plugins.get(&instance_id) {
            Some(p) => p,
            None => return false,
        };

        if plugin_data.preset != uri || !std::path::Path::new(bundlepath).exists() {
            return false;
        }

        let plugin_uri = plugin_data.uri.clone();

        // Remove bundle from lilv world
        if crate::lv2_utils::is_bundle_loaded(bundlepath) {
            self.ipc
                .send_and_wait_bool(&format!(
                    "bundle_remove \"{}\" \"{}\"",
                    bundlepath.replace('"', "\\\""),
                    uri.replace('"', "\\\"")
                ))
                .await;
            crate::lv2_utils::remove_bundle_from_lilv_world(bundlepath, Some(uri));
        }

        // Delete the bundle directory
        let _ = std::fs::remove_dir_all(bundlepath);
        crate::lv2_utils::rescan_plugin_presets(&plugin_uri);

        if let Some(p) = self.plugins.get_mut(&instance_id) {
            p.preset = String::new();
        }

        msg_callback(&format!("preset {} null", instance));
        true
    }

    // -------------------------------------------------------------------------
    // Mute/Unmute

    /// Mute the audio output (disconnect monitor from playback).
    pub fn mute(&self) {
        let ports = lv2_utils::get_jack_hardware_ports(true, true);
        for port in &ports {
            let num = match port.split_once(':').and_then(|(_, r)| r.strip_prefix("playback_")) {
                Some(n) => n,
                None => continue,
            };
            let monitor_num = if self.swapped_audio_channels && (num == "1" || num == "2") {
                if num == "1" { "2" } else { "1" }
            } else {
                num
            };
            lv2_utils::disconnect_jack_ports(
                &format!("mod-monitor:out_{}", monitor_num),
                &format!("system:playback_{}", num),
            );
        }
        lv2_utils::disconnect_jack_ports("mod-monitor:out_1", "mod-peakmeter:in_3");
        lv2_utils::disconnect_jack_ports("mod-monitor:out_2", "mod-peakmeter:in_4");
    }

    /// Unmute the audio output.
    pub fn unmute(&self) {
        let ports = lv2_utils::get_jack_hardware_ports(true, true);
        for port in &ports {
            let num = match port.split_once(':').and_then(|(_, r)| r.strip_prefix("playback_")) {
                Some(n) => n,
                None => continue,
            };
            let monitor_num = if self.swapped_audio_channels && (num == "1" || num == "2") {
                if num == "1" { "2" } else { "1" }
            } else {
                num
            };
            lv2_utils::connect_jack_ports(
                &format!("mod-monitor:out_{}", monitor_num),
                &format!("system:playback_{}", num),
            );
        }
        lv2_utils::connect_jack_ports("mod-monitor:out_1", "mod-peakmeter:in_3");
        lv2_utils::connect_jack_ports("mod-monitor:out_2", "mod-peakmeter:in_4");
    }

    // -------------------------------------------------------------------------
    // CV addressing

    /// Register a plugin CV output port as an available CV actuator.
    /// Returns the operational mode string ("+", "-", or "b").
    pub fn cv_addressing_plugin_port_add(&mut self, uri: &str, name: &str) -> String {
        self.addressings.cv_port_names.insert(uri.to_string(), name.to_string());
        if !self.addressings.cv_addressings.contains_key(uri) {
            self.addressings.cv_addressings.insert(uri.to_string(), Vec::new());
        }
        self.get_cv_port_op_mode(uri)
    }

    /// Remove a plugin CV output port from available CV actuators.
    /// Unaddresses everything assigned to it.
    pub fn cv_addressing_plugin_port_remove(&mut self, uri: &str) -> bool {
        if !self.addressings.cv_addressings.contains_key(uri) {
            return false;
        }
        self.addressings.cv_addressings.remove(uri);
        self.addressings.cv_port_names.remove(uri);
        true
    }

    /// Determine the operational mode of a CV port based on its min/max ranges.
    fn get_cv_port_op_mode(&self, actuator_uri: &str) -> String {
        // URI format: /cv/graph/Instance/PortSymbol
        let cv_path = actuator_uri
            .strip_prefix("/cv")
            .unwrap_or(actuator_uri);
        if let Some((instance, port_symbol)) = cv_path.rsplit_once('/') {
            if let Some(instance_id) = self.mapper.get_id_without_creating(instance) {
                if let Some(plugin_data) = self.plugins.get(&instance_id) {
                    if let Some(info) = lv2_utils::get_plugin_info(&plugin_data.uri) {
                        if let Some(cv_outputs) = info
                            .pointer("/ports/cv/output")
                            .and_then(|v| v.as_array())
                        {
                            for port in cv_outputs {
                                let sym = port.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
                                if sym == port_symbol {
                                    let min = port.pointer("/ranges/minimum").and_then(|v| v.as_f64()).unwrap_or(0.0);
                                    let max = port.pointer("/ranges/maximum").and_then(|v| v.as_f64()).unwrap_or(1.0);
                                    if min < 0.0 && max <= 0.0 {
                                        return "-".to_string();
                                    }
                                    if min < 0.0 && max > 0.0 {
                                        return "b".to_string();
                                    }
                                    return "+".to_string();
                                }
                            }
                        }
                    }
                }
            }
        }
        "+".to_string()
    }

    // -------------------------------------------------------------------------
    // Transport sync

    /// Enable Ableton Link sync.
    pub async fn set_link_enabled(&mut self) {
        self.ipc.send_notmodified("transport_sync link", None, "boolean").await;
        self.transport.sync = transport::SyncMode::AbletonLink;
        self.profile.set_sync_mode(2); // TRANSPORT_SOURCE_ABLETON_LINK
    }

    /// Enable MIDI clock slave sync.
    pub async fn set_midi_clock_slave_enabled(&mut self) {
        self.ipc.send_notmodified("transport_sync midi", None, "boolean").await;
        self.transport.sync = transport::SyncMode::MidiSlave;
        self.profile.set_sync_mode(1); // TRANSPORT_SOURCE_MIDI_SLAVE
    }

    /// Set internal transport source (disable external sync).
    pub async fn set_internal_transport_source(&mut self) {
        self.ipc.send_notmodified("feature_enable link 0", None, "boolean").await;
        self.ipc.send_notmodified("feature_enable midi_clock_slave 0", None, "boolean").await;
        self.transport.sync = transport::SyncMode::Internal;
        self.profile.set_sync_mode(0); // TRANSPORT_SOURCE_INTERNAL
    }

    // -------------------------------------------------------------------------
    // Session lifecycle

    /// Start a UI session (called when first websocket connects).
    /// Returns the read stream if a new connection was established.
    pub async fn start_session(&mut self) -> Option<tokio::net::TcpStream> {
        let read_stream = self.open_connection_if_needed().await;
        self.web_connected = true;

        // Only clear state on first connection (not on browser reload)
        if read_stream.is_some() {
            self.ipc.send_notmodified("remove -1", None, "int").await;
            self.mapper.clear();
            self.plugins = plugin::init_plugins_data();
            self.connections.clear();

            // Enable processing with data-ready signaling
            self.ipc
                .send_notmodified("feature_enable processing 2", None, "boolean")
                .await;

            // Enable MIDI program change monitoring on all 16 channels
            for ch in 0..16 {
                self.ipc
                    .send_notmodified(&format!("monitor_midi_program {} 1", ch), None, "boolean")
                    .await;
            }

            tracing::debug!("[host] cleared previous session state, processing and MIDI monitoring enabled");
        }

        read_stream
    }

    /// End a UI session (called when last websocket disconnects).
    pub async fn end_session(&mut self) {
        self.web_connected = false;
    }

    // -------------------------------------------------------------------------
    // Pedalboard save/load

    /// Save the current pedalboard state to TTL files.
    pub fn save_state_to_ttl(&self, bundlepath: &str, title: &str, titlesym: &str) {
        self.save_state_manifest(bundlepath, titlesym);
        self.save_state_snapshots(bundlepath);
        self.save_state_mainfile(bundlepath, title, titlesym);
    }

    fn save_state_manifest(&self, bundlepath: &str, titlesym: &str) {
        let content = format!(
            r#"@prefix ingen: <http://drobilla.net/ns/ingen#> .
@prefix lv2:   <http://lv2plug.in/ns/lv2core#> .
@prefix pedal: <http://moddevices.com/ns/modpedal#> .
@prefix rdfs:  <http://www.w3.org/2000/01/rdf-schema#> .

<{titlesym}.ttl>
    lv2:prototype ingen:GraphPrototype ;
    a lv2:Plugin ,
        ingen:Graph ,
        pedal:Pedalboard ;
    rdfs:seeAlso <{titlesym}.ttl> .
"#
        );
        let path = std::path::Path::new(bundlepath).join("manifest.ttl");
        if let Err(e) = utils::text_file_flusher(&path, &content) {
            tracing::error!("[host] failed to write manifest.ttl: {}", e);
        }
    }

    fn save_state_snapshots(&self, bundlepath: &str) {
        let snapshots: Vec<Value> = self
            .pedalboard
            .snapshots
            .iter()
            .filter_map(|s| {
                let s = s.as_ref()?;
                let mut data = serde_json::Map::new();
                for (&instance_id, ports) in &s.data {
                    if let Some(plugin) = self.plugins.get(&instance_id) {
                        let instance_key = plugin
                            .instance
                            .strip_prefix("/graph/")
                            .unwrap_or(&plugin.instance);
                        let ports_map: serde_json::Map<String, Value> = ports
                            .iter()
                            .map(|(k, v)| (k.clone(), Value::from(*v)))
                            .collect();
                        data.insert(
                            instance_key.to_string(),
                            serde_json::json!({
                                "bypassed": plugin.bypassed,
                                "ports": ports_map,
                                "preset": plugin.preset,
                            }),
                        );
                    }
                }
                Some(serde_json::json!({
                    "name": s.name,
                    "data": data,
                }))
            })
            .collect();

        let data = serde_json::json!({
            "current": self.pedalboard.current_snapshot_id,
            "snapshots": snapshots,
        });

        let path = std::path::Path::new(bundlepath).join("snapshots.json");
        let json_str = serde_json::to_string_pretty(&data).unwrap_or_default();
        if let Err(e) = utils::text_file_flusher(&path, &json_str) {
            tracing::error!("[host] failed to write snapshots.json: {}", e);
        }
    }

    fn save_state_mainfile(&self, bundlepath: &str, title: &str, titlesym: &str) {
        let mut arcs = String::new();
        let mut arc_index = 0;
        for conn in &self.connections.connections {
            arc_index += 1;
            let from = conn
                .port_from
                .strip_prefix("/graph/")
                .unwrap_or(&conn.port_from);
            let to = conn
                .port_to
                .strip_prefix("/graph/")
                .unwrap_or(&conn.port_to);
            arcs += &format!(
                "\n_:b{arc_index}\n    ingen:tail <{from}> ;\n    ingen:head <{to}> .\n"
            );
        }

        let mut blocks = String::new();
        for (&instance_id, plugin_data) in &self.plugins {
            if instance_id == PEDALBOARD_INSTANCE_ID {
                continue;
            }
            let instance = plugin_data
                .instance
                .strip_prefix("/graph/")
                .unwrap_or(&plugin_data.instance);

            // Get plugin info for version data
            let info = crate::lv2_utils::get_plugin_info(&plugin_data.uri);
            let micro_version = info
                .as_ref()
                .and_then(|i| i.get("microVersion").and_then(|v| v.as_i64()))
                .unwrap_or(0);
            let minor_version = info
                .as_ref()
                .and_then(|i| i.get("minorVersion").and_then(|v| v.as_i64()))
                .unwrap_or(0);
            let builder = info
                .as_ref()
                .and_then(|i| i.get("builder").and_then(|v| v.as_i64()))
                .unwrap_or(0);
            let release = info
                .as_ref()
                .and_then(|i| i.get("release").and_then(|v| v.as_i64()))
                .unwrap_or(0);

            // Collect all port symbols for the lv2:port line
            let mut port_syms: Vec<String> = Vec::new();
            if let Some(ref info) = info {
                if let Some(ports) = info.get("ports").and_then(|p| p.as_object()) {
                    for category in ["audio", "control", "cv", "midi"] {
                        if let Some(cat) = ports.get(category).and_then(|c| c.as_object()) {
                            for direction in ["input", "output"] {
                                if let Some(arr) = cat.get(direction).and_then(|d| d.as_array()) {
                                    for port in arr {
                                        if let Some(sym) =
                                            port.get("symbol").and_then(|s| s.as_str())
                                        {
                                            port_syms
                                                .push(format!("{instance}/{sym}"));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            port_syms.push(format!("{instance}/:bypass"));

            let port_list = port_syms
                .iter()
                .map(|s| format!("<{s}>"))
                .collect::<Vec<_>>()
                .join(" ,\n             ");

            blocks += &format!(
                r#"
<{instance}>
    ingen:canvasX {:.1} ;
    ingen:canvasY {:.1} ;
    ingen:enabled {} ;
    ingen:polyphonic false ;
    lv2:microVersion {micro_version} ;
    lv2:minorVersion {minor_version} ;
    mod:builderVersion {builder} ;
    mod:releaseNumber {release} ;
    lv2:port {port_list} ;
    lv2:prototype <{}> ;
    pedal:instanceNumber {instance_id} ;
    pedal:preset <{}> ;
    a ingen:Block .
"#,
                plugin_data.x as f64,
                plugin_data.y as f64,
                if plugin_data.bypassed { "false" } else { "true" },
                plugin_data.uri,
                plugin_data.preset,
            );

            // Write port definitions based on plugin info
            if let Some(ref info) = info {
                if let Some(ports) = info.get("ports").and_then(|p| p.as_object()) {
                    // Audio ports
                    for direction in ["input", "output"] {
                        if let Some(arr) = ports
                            .get("audio")
                            .and_then(|c| c.get(direction))
                            .and_then(|d| d.as_array())
                        {
                            let port_type = if direction == "input" {
                                "lv2:InputPort"
                            } else {
                                "lv2:OutputPort"
                            };
                            for port in arr {
                                if let Some(sym) = port.get("symbol").and_then(|s| s.as_str()) {
                                    blocks += &format!(
                                        "\n<{instance}/{sym}>\n    a lv2:AudioPort ,\n        {port_type} .\n"
                                    );
                                }
                            }
                        }
                    }
                    // CV ports
                    for direction in ["input", "output"] {
                        if let Some(arr) = ports
                            .get("cv")
                            .and_then(|c| c.get(direction))
                            .and_then(|d| d.as_array())
                        {
                            let port_type = if direction == "input" {
                                "lv2:InputPort"
                            } else {
                                "lv2:OutputPort"
                            };
                            for port in arr {
                                if let Some(sym) = port.get("symbol").and_then(|s| s.as_str()) {
                                    blocks += &format!(
                                        "\n<{instance}/{sym}>\n    a lv2:CVPort ,\n        {port_type} .\n"
                                    );
                                }
                            }
                        }
                    }
                    // MIDI ports
                    for direction in ["input", "output"] {
                        if let Some(arr) = ports
                            .get("midi")
                            .and_then(|c| c.get(direction))
                            .and_then(|d| d.as_array())
                        {
                            let port_type = if direction == "input" {
                                "lv2:InputPort"
                            } else {
                                "lv2:OutputPort"
                            };
                            for port in arr {
                                if let Some(sym) = port.get("symbol").and_then(|s| s.as_str()) {
                                    blocks += &format!(
                                        "\n<{instance}/{sym}>\n    atom:bufferType atom:Sequence ;\n    atom:supports midi:MidiEvent ;\n    a atom:AtomPort ,\n        {port_type} .\n"
                                    );
                                }
                            }
                        }
                    }
                }
            }

            // Control input ports with saved values
            for (symbol, &value) in &plugin_data.ports {
                let midi_binding =
                    if let Some(&(ch, ctrl, min, max)) = plugin_data.midi_ccs.get(symbol) {
                        if ch >= 0 && ctrl >= 0 {
                            format!(
                                "\n    midi:binding [\n        midi:channel {ch} ;\n        midi:controllerNumber {ctrl} ;\n        lv2:minimum {min:?} ;\n        lv2:maximum {max:?} ;\n        a midi:Controller ;\n    ] ;"
                            )
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    };
                blocks += &format!(
                    "\n<{instance}/{symbol}>\n    ingen:value {value:?} ;{midi_binding}\n    a lv2:ControlPort ,\n        lv2:InputPort .\n"
                );
            }

            // Control output ports
            if let Some(ref info) = info {
                if let Some(arr) = info
                    .get("ports")
                    .and_then(|p| p.get("control"))
                    .and_then(|c| c.get("output"))
                    .and_then(|d| d.as_array())
                {
                    for port in arr {
                        if let Some(sym) = port.get("symbol").and_then(|s| s.as_str()) {
                            blocks += &format!(
                                "\n<{instance}/{sym}>\n    a lv2:ControlPort ,\n        lv2:OutputPort .\n"
                            );
                        }
                    }
                }
            }

            // Bypass port
            let bypass_val = if plugin_data.bypassed { 1 } else { 0 };
            let bypass_midi = if plugin_data.bypass_cc.0 >= 0 && plugin_data.bypass_cc.1 >= 0 {
                format!(
                    "\n    midi:binding [\n        midi:channel {} ;\n        midi:controllerNumber {} ;\n        a midi:Controller ;\n    ] ;",
                    plugin_data.bypass_cc.0, plugin_data.bypass_cc.1
                )
            } else {
                String::new()
            };
            blocks += &format!(
                "\n<{instance}/:bypass>\n    ingen:value {bypass_val} ;{bypass_midi}\n    a lv2:ControlPort ,\n        lv2:InputPort .\n"
            );
        }

        // Global ports
        let ports = format!(
            r#"
<:bpb>
    ingen:value {:?} ;
    lv2:index 0 ;
    a lv2:ControlPort ,
        lv2:InputPort .

<:bpm>
    ingen:value {:?} ;
    lv2:index 1 ;
    a lv2:ControlPort ,
        lv2:InputPort .

<:rolling>
    ingen:value {} ;
    lv2:index 2 ;
    a lv2:ControlPort ,
        lv2:InputPort .

<control_in>
    atom:bufferType atom:Sequence ;
    lv2:index 3 ;
    lv2:name "Control In" ;
    lv2:portProperty lv2:connectionOptional ;
    lv2:symbol "control_in" ;
    <http://lv2plug.in/ns/ext/resize-port#minimumSize> 4096 ;
    a atom:AtomPort ,
        lv2:InputPort .

<control_out>
    atom:bufferType atom:Sequence ;
    lv2:index 4 ;
    lv2:name "Control Out" ;
    lv2:portProperty lv2:connectionOptional ;
    lv2:symbol "control_out" ;
    <http://lv2plug.in/ns/ext/resize-port#minimumSize> 4096 ;
    a atom:AtomPort ,
        lv2:OutputPort .

<midi_separated_mode>
    ingen:value {} ;
    lv2:index 5 ;
    a atom:AtomPort ,
        lv2:InputPort .

<midi_loopback>
    ingen:value {} ;
    lv2:index 6 ;
    a atom:AtomPort ,
        lv2:InputPort .
"#,
            self.transport.bpb,
            self.transport.bpm,
            if self.transport.rolling { 1 } else { 0 },
            if self.midi_aggregated_mode { 0 } else { 1 },
            if self.midi_loopback_enabled { 1 } else { 0 },
        );

        // Build the arc references
        let arc_refs = if arc_index > 0 {
            let refs: Vec<String> = (1..=arc_index).map(|i| format!("_:b{i}")).collect();
            format!("    ingen:arc {} ;\n", refs.join(" ,\n              "))
        } else {
            String::new()
        };

        // Build block references
        let block_instances: Vec<String> = self
            .plugins
            .iter()
            .filter(|(id, _)| **id != PEDALBOARD_INSTANCE_ID)
            .map(|(_, p)| {
                let inst = p
                    .instance
                    .strip_prefix("/graph/")
                    .unwrap_or(&p.instance);
                format!("<{inst}>")
            })
            .collect();
        let block_refs = if !block_instances.is_empty() {
            format!(
                "    ingen:block {} ;\n",
                block_instances.join(" ,\n                ")
            )
        } else {
            String::new()
        };

        // Port symbols list
        let port_syms = vec![
            ":bpb",
            ":bpm",
            ":rolling",
            "midi_separated_mode",
            "midi_loopback",
            "control_in",
            "control_out",
        ];
        let port_refs = format!(
            "    lv2:port {} ;\n",
            port_syms
                .iter()
                .map(|s| format!("<{s}>"))
                .collect::<Vec<_>>()
                .join(" ,\n             ")
        );

        let unit_name = self
            .descriptor
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown");
        let model_type = std::env::var("MOD_MODEL_TYPE").unwrap_or_else(|_| "Unknown".into());
        let escaped_title = title.replace('"', "\\\"");

        let pbdata = format!(
            r#"@prefix atom:  <http://lv2plug.in/ns/ext/atom#> .
@prefix doap:  <http://usefulinc.com/ns/doap#> .
@prefix ingen: <http://drobilla.net/ns/ingen#> .
@prefix lv2:   <http://lv2plug.in/ns/lv2core#> .
@prefix midi:  <http://lv2plug.in/ns/ext/midi#> .
@prefix mod:   <http://moddevices.com/ns/mod#> .
@prefix pedal: <http://moddevices.com/ns/modpedal#> .
@prefix rdfs:  <http://www.w3.org/2000/01/rdf-schema#> .
{arcs}{blocks}{ports}
<>
    doap:name "{escaped_title}" ;
    pedal:unitName "{unit_name}" ;
    pedal:unitModel "{model_type}" ;
    pedal:width {} ;
    pedal:height {} ;
    pedal:addressings <addressings.json> ;
    pedal:screenshot <screenshot.png> ;
    pedal:thumbnail <thumbnail.png> ;
    pedal:version {} ;
    ingen:polyphony 1 ;
{arc_refs}{block_refs}{port_refs}    lv2:extensionData <http://lv2plug.in/ns/ext/state#interface> ;
    a lv2:Plugin ,
        ingen:Graph ,
        pedal:Pedalboard .
"#,
            self.pedalboard.size.0,
            self.pedalboard.size.1,
            self.pedalboard.version,
        );

        let path =
            std::path::Path::new(bundlepath).join(format!("{titlesym}.ttl"));
        if let Err(e) = utils::text_file_flusher(&path, &pbdata) {
            tracing::error!("[host] failed to write {}.ttl: {}", titlesym, e);
        }
    }

    /// Load plugins from parsed pedalboard info into mod-host.
    pub async fn load_pb_plugins(&mut self, plugins: &[Value], msg_callback: &dyn Fn(&str)) {
        for p in plugins {
            let uri = p.get("uri").and_then(|v| v.as_str()).unwrap_or("");
            let instance_name = p.get("instance").and_then(|v| v.as_str()).unwrap_or("");
            let instance = format!("/graph/{}", instance_name);
            let instance_number = p
                .get("instanceNumber")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32;
            let bypassed = p.get("bypassed").and_then(|v| v.as_bool()).unwrap_or(false);
            let x = p.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let y = p.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let preset = p
                .get("preset")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let instance_id = self.mapper.get_id_by_number(&instance, instance_number);

            // Create plugin data
            let mut plugin_data = PluginData::new(&instance, uri, x as i32, y as i32);
            plugin_data.bypassed = bypassed;
            plugin_data.preset = preset.clone();

            let bypass_ch = p
                .get("bypassCC")
                .and_then(|v| v.get("channel"))
                .and_then(|v| v.as_i64())
                .unwrap_or(-1) as i32;
            let bypass_ctrl = p
                .get("bypassCC")
                .and_then(|v| v.get("control"))
                .and_then(|v| v.as_i64())
                .unwrap_or(-1) as i32;
            plugin_data.bypass_cc = (bypass_ch, bypass_ctrl);

            // Get plugin info for building version string
            let info = crate::lv2_utils::get_plugin_info(uri);
            if let Some(ref info) = info {
                plugin_data.build_env = info
                    .get("buildEnvironment")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                plugin_data.sversion = format!(
                    "{}_{}_{}_{}",
                    info.get("builder").and_then(|v| v.as_i64()).unwrap_or(0),
                    info.get("microVersion").and_then(|v| v.as_i64()).unwrap_or(0),
                    info.get("minorVersion").and_then(|v| v.as_i64()).unwrap_or(0),
                    info.get("release").and_then(|v| v.as_i64()).unwrap_or(0),
                );
            }

            // Send add command to mod-host
            self.ipc
                .send_notmodified(&format!("add {} {}", uri, instance_id), None, "int")
                .await;

            if bypassed {
                self.ipc
                    .send_notmodified(&format!("bypass {} 1", instance_id), None, "int")
                    .await;
            }

            // Notify websocket clients
            let has_build_env = if plugin_data.build_env.is_empty() { 0 } else { 1 };
            msg_callback(&format!(
                "add {} {} {:.1} {:.1} {} {} {}",
                instance,
                uri,
                x,
                y,
                if bypassed { 1 } else { 0 },
                plugin_data.sversion,
                has_build_env,
            ));

            // Load preset if any
            if !preset.is_empty() {
                self.ipc
                    .send_notmodified(
                        &format!("preset_load {} {}", instance_id, preset),
                        None,
                        "int",
                    )
                    .await;
                msg_callback(&format!("preset {} {}", instance, preset));
            }

            // Set port values from the saved pedalboard
            if let Some(ports) = p.get("ports").and_then(|v| v.as_array()) {
                for port in ports {
                    let symbol = port.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
                    let value = port.get("value").and_then(|v| v.as_f64()).unwrap_or(0.0);

                    plugin_data.ports.insert(symbol.to_string(), value);

                    self.ipc
                        .send_notmodified(
                            &format!("param_set {} {} {}", instance_id, symbol, value),
                            None,
                            "int",
                        )
                        .await;
                    msg_callback(&format!("param_set {} {} {}", instance, symbol, value));

                    // MIDI CC bindings
                    if let Some(midi_cc) = port.get("midiCC") {
                        let ch = midi_cc
                            .get("channel")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(-1) as i32;
                        let ctrl = midi_cc
                            .get("control")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(-1) as i32;
                        if ch >= 0 && ch < 16 && ctrl >= 0 {
                            let has_ranges = midi_cc
                                .get("hasRanges")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            let minimum = if has_ranges {
                                midi_cc
                                    .get("minimum")
                                    .and_then(|v| v.as_f64())
                                    .unwrap_or(0.0)
                            } else {
                                0.0
                            };
                            let maximum = if has_ranges {
                                midi_cc
                                    .get("maximum")
                                    .and_then(|v| v.as_f64())
                                    .unwrap_or(1.0)
                            } else {
                                1.0
                            };
                            plugin_data
                                .midi_ccs
                                .insert(symbol.to_string(), (ch, ctrl, minimum, maximum));
                            self.ipc
                                .send_notmodified(
                                    &format!(
                                        "midi_map {} {} {} {} {} {}",
                                        instance_id, symbol, ch, ctrl, minimum, maximum
                                    ),
                                    None,
                                    "boolean",
                                )
                                .await;
                        }
                    }
                }
            }

            // Monitor outputs
            if let Some(ref info) = info {
                if let Some(monitored) = info
                    .get("gui")
                    .and_then(|g| g.get("monitoredOutputs"))
                    .and_then(|v| v.as_array())
                {
                    for output in monitored {
                        if let Some(sym) = output.as_str() {
                            plugin_data
                                .outputs
                                .insert(sym.to_string(), None);
                            self.ipc
                                .send_notmodified(
                                    &format!("monitor_output {} {}", instance_id, sym),
                                    None,
                                    "int",
                                )
                                .await;
                        }
                    }
                }
            }

            // Populate default values for all control input ports not already set
            if let Some(ref info) = info {
                plugin_data.populate_port_defaults(info);
            }

            // Populate parameters (file paths, atom properties) from plugin info
            if let Some(ref info) = info {
                if let Some(params) = info.get("parameters").and_then(|v| v.as_array()) {
                    for param in params {
                        let param_uri = param.get("uri").and_then(|v| v.as_str()).unwrap_or("");
                        let writable = param.get("writable").and_then(|v| v.as_bool()).unwrap_or(false);
                        if param_uri.is_empty() || !writable {
                            continue;
                        }
                        let type_uri = param.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        let type_char = atom_type_char(type_uri).to_string();
                        let default = param.get("ranges")
                            .and_then(|r| r.get("default"))
                            .cloned()
                            .unwrap_or(serde_json::Value::String(String::new()));
                        plugin_data.parameters.insert(param_uri.to_string(), (default, type_char));
                    }
                }
            }

            self.plugins.insert(instance_id, plugin_data);

            // Send patch_get for each writable parameter to retrieve current values from mod-host
            if let Some(plugin_data) = self.plugins.get(&instance_id) {
                let param_uris: Vec<String> = plugin_data.parameters.keys().cloned().collect();
                for param_uri in param_uris {
                    self.ipc
                        .send_notmodified(
                            &format!("patch_get {} {}", instance_id, param_uri),
                            None,
                            "boolean",
                        )
                        .await;
                }
            }
        }
    }

    /// Load connections from parsed pedalboard info.
    pub async fn load_pb_connections(
        &mut self,
        connections: &[Value],
        msg_callback: &dyn Fn(&str),
    ) {
        for c in connections {
            let source = c.get("source").and_then(|v| v.as_str()).unwrap_or("");
            let target = c.get("target").and_then(|v| v.as_str()).unwrap_or("");
            if source.is_empty() || target.is_empty() {
                continue;
            }

            let port_from = format!("/graph/{}", source);
            let port_to = format!("/graph/{}", target);

            self.connections.add(&port_from, &port_to);
            let jack_from = self.fix_host_connection_port(&port_from);
            let jack_to = self.fix_host_connection_port(&port_to);
            self.ipc
                .send_notmodified(
                    &format!("connect {} {}", jack_from, jack_to),
                    None,
                    "int",
                )
                .await;
            msg_callback(&format!("connect {} {}", port_from, port_to));
        }
    }

    /// Load snapshots from the bundle's snapshots.json file.
    pub fn load_pb_snapshots(&mut self, bundlepath: &str) {
        let snapshots_path = std::path::Path::new(bundlepath).join("snapshots.json");
        if snapshots_path.exists() {
            let data: serde_json::Map<String, Value> = utils::safe_json_load(&snapshots_path);
            let current = data
                .get("current")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32;

            if let Some(snapshots) = data.get("snapshots").and_then(|v| v.as_array()) {
                self.pedalboard.snapshots.clear();
                for snap in snapshots {
                    let name = snap
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Snapshot")
                        .to_string();
                    // Parse snapshot data (instance -> ports)
                    let mut snap_data =
                        std::collections::HashMap::new();
                    if let Some(data_obj) = snap.get("data").and_then(|v| v.as_object()) {
                        for (instance_name, instance_data) in data_obj {
                            let full_instance = format!("/graph/{}", instance_name);
                            if let Some(id) =
                                self.mapper.get_id_without_creating(&full_instance)
                            {
                                let mut port_values = std::collections::HashMap::new();
                                if let Some(ports) =
                                    instance_data.get("ports").and_then(|v| v.as_object())
                                {
                                    for (port_sym, val) in ports {
                                        if let Some(v) = val.as_f64() {
                                            port_values.insert(port_sym.clone(), v);
                                        }
                                    }
                                }
                                snap_data.insert(id, port_values);
                            }
                        }
                    }

                    self.pedalboard.snapshots.push(Some(
                        pedalboard::Snapshot {
                            name,
                            data: snap_data,
                            plugins_added: Vec::new(),
                        },
                    ));
                }
            }

            if current >= 0 && (current as usize) < self.pedalboard.snapshots.len() {
                self.pedalboard.current_snapshot_id = current;
            } else {
                self.pedalboard.current_snapshot_id = 0;
            }
        } else {
            self.pedalboard.current_snapshot_id = -1;
        }
    }

    // -------------------------------------------------------------------------
    // State reporting

    /// Get current system stats as a websocket message.
    /// Format: "sys_stats <mem_usage_%> <cpu_freq_khz> <cpu_temp_milli_c>"
    pub fn get_system_stats_message(&self) -> String {
        let memload = get_memory_usage_percent();
        let cpufreq = read_sys_file("/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq");
        let cputemp = read_sys_file("/sys/class/thermal/thermal_zone0/temp");
        format!("sys_stats {} {} {}", memload, cpufreq, cputemp)
    }
}

/// Read a single-line sysfs file, returning "0" on failure.
fn read_sys_file(path: &str) -> String {
    std::fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

/// Calculate memory usage percentage from /proc/meminfo.
fn get_memory_usage_percent() -> String {
    let contents = match std::fs::read_to_string("/proc/meminfo") {
        Ok(c) => c,
        Err(_) => return "??".to_string(),
    };

    let mut mem_total: f64 = 0.0;
    let mut mem_free: f64 = 0.0;
    let mut buffers: f64 = 0.0;
    let mut cached: f64 = 0.0;
    let mut shmem: f64 = 0.0;
    let mut s_reclaimable: f64 = 0.0;

    for line in contents.lines() {
        let parse_kb = |l: &str| -> f64 {
            l.split_whitespace()
                .nth(1)
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0)
        };
        if line.starts_with("MemTotal:") {
            mem_total = parse_kb(line);
        } else if line.starts_with("MemFree:") {
            mem_free = parse_kb(line);
        } else if line.starts_with("Buffers:") {
            buffers = parse_kb(line);
        } else if line.starts_with("Cached:") {
            cached = parse_kb(line);
        } else if line.starts_with("Shmem:") {
            shmem = parse_kb(line);
        } else if line.starts_with("SReclaimable:") {
            s_reclaimable = parse_kb(line);
        }
    }

    if mem_total == 0.0 {
        return "??".to_string();
    }

    let mem_cached = buffers + cached - shmem + s_reclaimable;
    let used_pct = (mem_total - mem_free - mem_cached) / mem_total * 100.0;
    format!("{:.0}", used_pct)
}

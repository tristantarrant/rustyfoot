// Plugin data structures and management.
// Ported from the plugin-related parts of mod/host.py

use std::collections::HashMap;

use crate::settings::{PEDALBOARD_INSTANCE, PEDALBOARD_INSTANCE_ID, PEDALBOARD_URI};

/// Designation indices for special LV2 control ports.
pub const DESIGNATIONS_INDEX_ENABLED: usize = 0;
pub const DESIGNATIONS_INDEX_FREEWHEEL: usize = 1;
pub const DESIGNATIONS_INDEX_BPB: usize = 2;
pub const DESIGNATIONS_INDEX_BPM: usize = 3;
pub const DESIGNATIONS_INDEX_SPEED: usize = 4;

/// Plugin port designations (special-purpose ports).
#[derive(Debug, Clone, Default)]
pub struct Designations {
    pub enabled: Option<String>,
    pub freewheel: Option<String>,
    pub bpb: Option<String>,
    pub bpm: Option<String>,
    pub speed: Option<String>,
}

/// MIDI CC mapping for a port: (channel, controller, min, max).
pub type MidiCC = (i32, i32, f64, f64);

/// Data for a single plugin instance.
#[derive(Debug, Clone)]
pub struct PluginData {
    pub instance: String,
    pub uri: String,
    pub bypassed: bool,
    pub bypass_cc: (i32, i32),
    pub x: i32,
    pub y: i32,
    pub addressings: HashMap<String, serde_json::Value>,
    pub midi_ccs: HashMap<String, MidiCC>,
    pub ports: HashMap<String, f64>,
    pub parameters: HashMap<String, (serde_json::Value, String)>, // (default_value, type_char)
    pub ranges: HashMap<String, (f64, f64)>,                       // symbol -> (min, max)
    pub bad_ports: Vec<String>,
    pub designations: Designations,
    pub outputs: HashMap<String, Option<f64>>,
    pub preset: String,
    pub map_presets: Vec<String>,
    pub next_preset: String,
    pub build_env: String,
    pub sversion: String,
}

impl PluginData {
    /// Populate `ports` with default values for all control input ports from plugin info.
    /// This ensures snapshots capture all port values, not just explicitly changed ones.
    pub fn populate_port_defaults(&mut self, info: &serde_json::Value) {
        if let Some(inputs) = info
            .get("ports")
            .and_then(|p| p.get("control"))
            .and_then(|c| c.get("input"))
            .and_then(|i| i.as_array())
        {
            for port in inputs {
                let symbol = port.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
                if symbol.is_empty() {
                    continue;
                }
                let default = port
                    .get("ranges")
                    .and_then(|r| r.get("default"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                // Only insert if not already set (saved values take priority)
                self.ports.entry(symbol.to_string()).or_insert(default);
            }
        }
    }

    /// Create a new plugin data with default values.
    pub fn new(instance: &str, uri: &str, x: i32, y: i32) -> Self {
        Self {
            instance: instance.to_string(),
            uri: uri.to_string(),
            bypassed: false,
            bypass_cc: (-1, -1),
            x,
            y,
            addressings: HashMap::new(),
            midi_ccs: HashMap::new(),
            ports: HashMap::new(),
            parameters: HashMap::new(),
            ranges: HashMap::new(),
            bad_ports: Vec::new(),
            designations: Designations::default(),
            outputs: HashMap::new(),
            preset: String::new(),
            map_presets: Vec::new(),
            next_preset: String::new(),
            build_env: String::new(),
            sversion: String::new(),
        }
    }
}

/// Plugin storage: instance_id -> PluginData.
pub type PluginMap = HashMap<i32, PluginData>;

/// Create the initial plugins map with only the pedalboard pseudo-instance.
pub fn init_plugins_data() -> PluginMap {
    let mut plugins = HashMap::new();

    let mut pedalboard = PluginData::new(PEDALBOARD_INSTANCE, PEDALBOARD_URI, 0, 0);
    pedalboard.midi_ccs.insert(":bpb".into(), (-1, -1, 0.0, 1.0));
    pedalboard.midi_ccs.insert(":bpm".into(), (-1, -1, 0.0, 1.0));
    pedalboard
        .midi_ccs
        .insert(":rolling".into(), (-1, -1, 0.0, 1.0));

    plugins.insert(PEDALBOARD_INSTANCE_ID, pedalboard);
    plugins
}

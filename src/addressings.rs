// Control addressing system, ported from mod/addressings.py
// Maps plugin control ports to hardware actuators (HMI, MIDI, CV, BPM).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

use crate::utils;

// Addressing type constants
pub const ADDRESSING_TYPE_NONE: i32 = 0;
pub const ADDRESSING_TYPE_HMI: i32 = 1;
pub const ADDRESSING_TYPE_CC: i32 = 2;
pub const ADDRESSING_TYPE_MIDI: i32 = 3;
pub const ADDRESSING_TYPE_BPM: i32 = 4;
pub const ADDRESSING_TYPE_CV: i32 = 5;

// Special actuator URIs
pub const ACTUATOR_URI_NULL: &str = "null";
pub const ACTUATOR_URI_MIDI_LEARN: &str = "/midi-learn";
pub const ACTUATOR_URI_BPM: &str = "/bpm";
pub const ACTUATOR_CV_PREFIX: &str = "/cv";
pub const ACTUATOR_HMI_PREFIX: &str = "/hmi";

/// Data for a single control addressing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddressingData {
    pub actuator_uri: String,
    pub instance_id: i32,
    pub port: String,
    pub label: String,
    pub value: f64,
    pub minimum: f64,
    pub maximum: f64,
    pub steps: i32,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub options: Vec<(f64, String)>,
    #[serde(default)]
    pub tempo: bool,
    #[serde(default)]
    pub dividers: Option<i32>,
    #[serde(default)]
    pub page: Option<i32>,
    #[serde(default)]
    pub subpage: Option<i32>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub coloured: bool,
    #[serde(default)]
    pub momentary: i32,
    #[serde(default)]
    pub operational_mode: String,
}

/// HMI actuator slot: holds a list of addressings and a current index.
#[derive(Debug, Clone, Default)]
pub struct HmiActuatorSlot {
    pub addrs: Vec<AddressingData>,
    pub idx: usize,
}

/// The main addressings manager.
pub struct Addressings {
    /// HMI addressings: actuator_uri -> slot
    pub hmi_addressings: HashMap<String, HmiActuatorSlot>,
    /// MIDI addressings: actuator_uri -> list
    pub midi_addressings: HashMap<String, Vec<AddressingData>>,
    /// Virtual addressings (BPM): actuator_uri -> list
    pub virtual_addressings: HashMap<String, Vec<AddressingData>>,
    /// CV addressings: actuator_uri -> list
    pub cv_addressings: HashMap<String, Vec<AddressingData>>,
    /// CV port names: actuator_uri -> display name
    pub cv_port_names: HashMap<String, String>,

    /// HMI hardware ID to URI mapping
    pub hmi_hw2uri_map: HashMap<i32, String>,
    /// HMI URI to hardware ID mapping
    pub hmi_uri2hw_map: HashMap<String, i32>,

    /// Available HMI actuator URIs
    pub hw_actuators_uris: Vec<String>,

    /// Current HMI page
    pub current_page: i32,
    /// Number of addressing pages
    pub addressing_pages: i32,
    /// Which pages have addressings
    pub available_pages: Vec<bool>,
}

impl Addressings {
    pub fn new() -> Self {
        Self {
            hmi_addressings: HashMap::new(),
            midi_addressings: HashMap::new(),
            virtual_addressings: HashMap::new(),
            cv_addressings: HashMap::new(),
            cv_port_names: HashMap::new(),
            hmi_hw2uri_map: HashMap::new(),
            hmi_uri2hw_map: HashMap::new(),
            hw_actuators_uris: Vec::new(),
            current_page: 0,
            addressing_pages: 0,
            available_pages: Vec::new(),
        }
    }

    /// Initialize from hardware descriptor.
    pub fn init(&mut self, hw_desc: &serde_json::Map<String, Value>) {
        // Extract actuators from hardware descriptor
        if let Some(actuators) = hw_desc.get("actuators").and_then(|v| v.as_array()) {
            for act in actuators {
                if let (Some(id), Some(uri)) = (
                    act.get("id").and_then(|v| v.as_i64()),
                    act.get("uri").and_then(|v| v.as_str()),
                ) {
                    let hw_id = id as i32;
                    let uri = uri.to_string();
                    self.hmi_hw2uri_map.insert(hw_id, uri.clone());
                    self.hmi_uri2hw_map.insert(uri.clone(), hw_id);
                    self.hw_actuators_uris.push(uri.clone());
                    self.hmi_addressings
                        .insert(uri, HmiActuatorSlot::default());
                }
            }
        }

        // Extract pages info
        self.addressing_pages = hw_desc
            .get("pages")
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32;
        self.available_pages = vec![false; self.addressing_pages.max(0) as usize];
    }

    /// Clear all addressings (but preserve hardware metadata).
    pub fn clear(&mut self) {
        for slot in self.hmi_addressings.values_mut() {
            slot.addrs.clear();
            slot.idx = 0;
        }
        self.midi_addressings.clear();
        self.virtual_addressings.clear();
        self.cv_addressings.clear();
        self.cv_port_names.clear();
        self.current_page = 0;
        for page in self.available_pages.iter_mut() {
            *page = false;
        }
    }

    /// Get the addressing type from an actuator URI.
    pub fn get_actuator_type(actuator_uri: &str) -> i32 {
        if actuator_uri.starts_with(ACTUATOR_HMI_PREFIX) {
            ADDRESSING_TYPE_HMI
        } else if actuator_uri.starts_with(ACTUATOR_CV_PREFIX) {
            ADDRESSING_TYPE_CV
        } else if actuator_uri == ACTUATOR_URI_BPM {
            ADDRESSING_TYPE_BPM
        } else if actuator_uri.starts_with("/midi") {
            ADDRESSING_TYPE_MIDI
        } else {
            ADDRESSING_TYPE_NONE
        }
    }

    /// Check if an actuator URI is an HMI actuator.
    pub fn is_hmi_actuator(actuator_uri: &str) -> bool {
        actuator_uri.starts_with(ACTUATOR_HMI_PREFIX)
    }

    /// Add an addressing.
    pub fn add(&mut self, data: AddressingData) {
        let uri = data.actuator_uri.clone();
        let addr_type = Self::get_actuator_type(&uri);

        match addr_type {
            ADDRESSING_TYPE_HMI => {
                if let Some(slot) = self.hmi_addressings.get_mut(&uri) {
                    slot.addrs.push(data);
                }
            }
            ADDRESSING_TYPE_MIDI => {
                self.midi_addressings.entry(uri).or_default().push(data);
            }
            ADDRESSING_TYPE_BPM => {
                self.virtual_addressings.entry(uri).or_default().push(data);
            }
            ADDRESSING_TYPE_CV => {
                self.cv_addressings.entry(uri).or_default().push(data);
            }
            _ => {
                tracing::warn!("[addressings] unknown actuator type for URI: {}", uri);
            }
        }
    }

    /// Remove an addressing by instance_id and port.
    pub fn remove(&mut self, instance_id: i32, port: &str) {
        // Remove from HMI
        for slot in self.hmi_addressings.values_mut() {
            slot.addrs
                .retain(|a| !(a.instance_id == instance_id && a.port == port));
            if slot.idx >= slot.addrs.len() && !slot.addrs.is_empty() {
                slot.idx = slot.addrs.len() - 1;
            }
        }

        // Remove from MIDI
        for addrs in self.midi_addressings.values_mut() {
            addrs.retain(|a| !(a.instance_id == instance_id && a.port == port));
        }

        // Remove from virtual
        for addrs in self.virtual_addressings.values_mut() {
            addrs.retain(|a| !(a.instance_id == instance_id && a.port == port));
        }

        // Remove from CV
        for addrs in self.cv_addressings.values_mut() {
            addrs.retain(|a| !(a.instance_id == instance_id && a.port == port));
        }
    }

    /// Remove all addressings for a given instance_id.
    pub fn remove_instance(&mut self, instance_id: i32) {
        for slot in self.hmi_addressings.values_mut() {
            slot.addrs.retain(|a| a.instance_id != instance_id);
            if slot.idx >= slot.addrs.len() && !slot.addrs.is_empty() {
                slot.idx = slot.addrs.len() - 1;
            }
        }
        for addrs in self.midi_addressings.values_mut() {
            addrs.retain(|a| a.instance_id != instance_id);
        }
        for addrs in self.virtual_addressings.values_mut() {
            addrs.retain(|a| a.instance_id != instance_id);
        }
        for addrs in self.cv_addressings.values_mut() {
            addrs.retain(|a| a.instance_id != instance_id);
        }
    }

    /// Get current addressing data for an HMI actuator (by hardware ID).
    pub fn hmi_get_addr_data(&self, hw_id: i32) -> Option<&AddressingData> {
        let uri = self.hmi_hw2uri_map.get(&hw_id)?;
        let slot = self.hmi_addressings.get(uri)?;
        slot.addrs.get(slot.idx)
    }

    /// Get available actuators info for the web UI.
    pub fn get_actuators(&self) -> Vec<Value> {
        let mut result = Vec::new();
        for uri in &self.hw_actuators_uris {
            if let Some(&hw_id) = self.hmi_uri2hw_map.get(uri) {
                result.push(serde_json::json!({
                    "uri": uri,
                    "hw_id": hw_id,
                }));
            }
        }
        result
    }

    /// Save addressings to a JSON file inside a pedalboard bundle.
    pub fn save(&self, bundlepath: &Path, instances: &HashMap<i32, String>) {
        let mut data: HashMap<String, Value> = HashMap::new();

        // Save HMI addressings
        for (uri, slot) in &self.hmi_addressings {
            if slot.addrs.is_empty() {
                continue;
            }
            let addrs: Vec<Value> = slot
                .addrs
                .iter()
                .filter_map(|a| {
                    let instance = instances.get(&a.instance_id)?;
                    Some(serde_json::json!({
                        "instance": instance,
                        "port": a.port,
                        "label": a.label,
                        "minimum": a.minimum,
                        "maximum": a.maximum,
                        "steps": a.steps,
                        "tempo": a.tempo,
                        "dividers": a.dividers,
                        "page": a.page,
                        "subpage": a.subpage,
                        "group": a.group,
                        "coloured": a.coloured,
                        "momentary": a.momentary,
                    }))
                })
                .collect();
            if !addrs.is_empty() {
                data.insert(uri.clone(), Value::Array(addrs));
            }
        }

        // Save MIDI addressings
        for (uri, addrs) in &self.midi_addressings {
            let items: Vec<Value> = addrs
                .iter()
                .filter_map(|a| {
                    let instance = instances.get(&a.instance_id)?;
                    Some(serde_json::json!({
                        "instance": instance,
                        "port": a.port,
                        "label": a.label,
                        "minimum": a.minimum,
                        "maximum": a.maximum,
                        "steps": a.steps,
                    }))
                })
                .collect();
            if !items.is_empty() {
                data.insert(uri.clone(), Value::Array(items));
            }
        }

        // Save virtual addressings (BPM)
        for (uri, addrs) in &self.virtual_addressings {
            let items: Vec<Value> = addrs
                .iter()
                .filter_map(|a| {
                    let instance = instances.get(&a.instance_id)?;
                    Some(serde_json::json!({
                        "instance": instance,
                        "port": a.port,
                        "label": a.label,
                        "minimum": a.minimum,
                        "maximum": a.maximum,
                        "steps": a.steps,
                    }))
                })
                .collect();
            if !items.is_empty() {
                data.insert(uri.clone(), Value::Array(items));
            }
        }

        let path = bundlepath.join("addressings.json");
        let json = serde_json::to_string_pretty(&data).unwrap_or_default();
        if let Err(e) = utils::text_file_flusher(&path, &json) {
            tracing::error!("[addressings] failed to save: {}", e);
        }
    }

    /// Create a MIDI CC actuator URI.
    pub fn create_midi_cc_uri(channel: i32, controller: i32) -> String {
        format!("/midi-custom_Ch.{}_CC#{}", channel + 1, controller)
    }

    /// Parse a MIDI CC actuator URI back to (channel, controller).
    pub fn get_midi_cc_from_uri(uri: &str) -> Option<(i32, i32)> {
        // Format: /midi-custom_Ch.N_CC#M
        let rest = uri.strip_prefix("/midi-custom_Ch.")?;
        let parts: Vec<&str> = rest.split("_CC#").collect();
        if parts.len() != 2 {
            return None;
        }
        let channel: i32 = parts[0].parse().ok()?;
        let controller: i32 = parts[1].parse().ok()?;
        Some((channel - 1, controller))
    }

    /// Update available pages based on current addressings.
    pub fn update_available_pages(&mut self) {
        for page in self.available_pages.iter_mut() {
            *page = false;
        }
        for slot in self.hmi_addressings.values() {
            for addr in &slot.addrs {
                if let Some(page) = addr.page {
                    if (page as usize) < self.available_pages.len() {
                        self.available_pages[page as usize] = true;
                    }
                }
            }
        }
    }

    /// Get pages that have at least one addressing.
    pub fn get_available_pages(&self) -> Vec<i32> {
        self.available_pages
            .iter()
            .enumerate()
            .filter(|(_, has)| **has)
            .map(|(i, _)| i as i32)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_actuator_type() {
        assert_eq!(
            Addressings::get_actuator_type("/hmi/knob1"),
            ADDRESSING_TYPE_HMI
        );
        assert_eq!(
            Addressings::get_actuator_type("/cv/graph/env/out"),
            ADDRESSING_TYPE_CV
        );
        assert_eq!(
            Addressings::get_actuator_type("/bpm"),
            ADDRESSING_TYPE_BPM
        );
        assert_eq!(
            Addressings::get_actuator_type("/midi-custom_Ch.1_CC#7"),
            ADDRESSING_TYPE_MIDI
        );
        assert_eq!(
            Addressings::get_actuator_type("unknown"),
            ADDRESSING_TYPE_NONE
        );
    }

    #[test]
    fn test_midi_cc_uri() {
        let uri = Addressings::create_midi_cc_uri(0, 7);
        assert_eq!(uri, "/midi-custom_Ch.1_CC#7");

        let (ch, cc) = Addressings::get_midi_cc_from_uri(&uri).unwrap();
        assert_eq!(ch, 0);
        assert_eq!(cc, 7);
    }

    #[test]
    fn test_add_remove() {
        let mut addr = Addressings::new();
        // Set up a fake HMI slot
        addr.hmi_addressings.insert(
            "/hmi/knob1".into(),
            HmiActuatorSlot::default(),
        );
        addr.hw_actuators_uris.push("/hmi/knob1".into());

        let data = AddressingData {
            actuator_uri: "/hmi/knob1".into(),
            instance_id: 1,
            port: "gain".into(),
            label: "Gain".into(),
            value: 0.5,
            minimum: 0.0,
            maximum: 1.0,
            steps: 100,
            unit: "dB".into(),
            options: vec![],
            tempo: false,
            dividers: None,
            page: Some(0),
            subpage: None,
            group: None,
            coloured: false,
            momentary: 0,
            operational_mode: String::new(),
        };

        addr.add(data);
        assert_eq!(addr.hmi_addressings["/hmi/knob1"].addrs.len(), 1);

        addr.remove(1, "gain");
        assert_eq!(addr.hmi_addressings["/hmi/knob1"].addrs.len(), 0);
    }
}

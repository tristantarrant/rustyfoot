use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Per-CC MIDI expression pedal calibration.
///
/// Compensates for physical pedals that don't reach the full 0–127 CC range
/// by adjusting the parameter min/max sent to mod-host so that the pedal's
/// actual CC range maps to the full parameter range.
#[derive(Clone)]
pub struct MidiCalibration {
    /// CC number → (cc_min, cc_max) — the actual range the pedal sends
    entries: HashMap<i32, (i32, i32)>,
    path: PathBuf,
}

impl MidiCalibration {
    pub fn new(data_dir: &Path) -> Self {
        let path = data_dir.join("midi_calibration.json");
        let entries = Self::load_from_file(&path);
        MidiCalibration { entries, path }
    }

    fn load_from_file(path: &Path) -> HashMap<i32, (i32, i32)> {
        let data = match std::fs::read_to_string(path) {
            Ok(d) => d,
            Err(_) => return HashMap::new(),
        };
        let json: serde_json::Value = match serde_json::from_str(&data) {
            Ok(v) => v,
            Err(_) => return HashMap::new(),
        };
        let mut entries = HashMap::new();
        if let Some(obj) = json.as_object() {
            for (key, val) in obj {
                if let Ok(cc) = key.parse::<i32>() {
                    let cc_min = val.get("cc_min").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    let cc_max = val.get("cc_max").and_then(|v| v.as_i64()).unwrap_or(127) as i32;
                    if cc_min != 0 || cc_max != 127 {
                        entries.insert(cc, (cc_min, cc_max));
                    }
                }
            }
        }
        entries
    }

    fn save(&self) {
        let mut map = serde_json::Map::new();
        for (&cc, &(cc_min, cc_max)) in &self.entries {
            let mut entry = serde_json::Map::new();
            entry.insert("cc_min".into(), serde_json::Value::Number(cc_min.into()));
            entry.insert("cc_max".into(), serde_json::Value::Number(cc_max.into()));
            map.insert(cc.to_string(), serde_json::Value::Object(entry));
        }
        let json = serde_json::Value::Object(map);
        if let Err(e) = std::fs::write(&self.path, serde_json::to_string_pretty(&json).unwrap_or_default()) {
            tracing::warn!("Failed to save MIDI calibration: {}", e);
        }
    }

    pub fn get_all(&self) -> &HashMap<i32, (i32, i32)> {
        &self.entries
    }

    pub fn set(&mut self, cc: i32, cc_min: i32, cc_max: i32) {
        if cc_min == 0 && cc_max == 127 {
            self.entries.remove(&cc);
        } else {
            self.entries.insert(cc, (cc_min, cc_max));
        }
        self.save();
    }

    pub fn remove(&mut self, cc: i32) {
        self.entries.remove(&cc);
        self.save();
    }

    /// Adjust parameter min/max to compensate for a pedal's actual CC range.
    ///
    /// Given a CC controller number and the desired parameter range, returns
    /// adjusted min/max values such that when mod-host maps CC 0–127 to
    /// adjusted_min–adjusted_max, the pedal's actual cc_min–cc_max range
    /// maps exactly to param_min–param_max.
    pub fn adjust(&self, cc: i32, param_min: f64, param_max: f64) -> (f64, f64) {
        let (cc_min, cc_max) = match self.entries.get(&cc) {
            Some(&v) => v,
            None => return (param_min, param_max),
        };
        if cc_min == 0 && cc_max == 127 {
            return (param_min, param_max);
        }
        let cc_range = (cc_max - cc_min) as f64;
        if cc_range <= 0.0 {
            return (param_min, param_max);
        }
        // Expand the range so that cc_min→param_min and cc_max→param_max
        let adjusted_range = (param_max - param_min) * 127.0 / cc_range;
        let adjusted_min = param_min - (cc_min as f64 / 127.0) * adjusted_range;
        let adjusted_max = adjusted_min + adjusted_range;
        (adjusted_min, adjusted_max)
    }
}

// Hardware profile/mixer settings, ported from mod/profile.py
// Manages persistent hardware profiles (volumes, MIDI, sync, etc.)

use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::settings::Settings;
use crate::utils;

// Sync mode constants
pub const SYNC_MODE_INTERNAL: i32 = 0;
pub const SYNC_MODE_MIDI_SLAVE: i32 = 1;
pub const SYNC_MODE_ABLETON_LINK: i32 = 2;

// Master volume channel modes
pub const MASTER_VOL_BOTH: i32 = 0;
pub const MASTER_VOL_CH1: i32 = 1;
pub const MASTER_VOL_CH2: i32 = 2;

// Expression pedal modes
pub const EXP_MODE_TIP: i32 = 0;
pub const EXP_MODE_RING: i32 = 1;

/// Default profile values.
fn default_values() -> HashMap<String, Value> {
    let mut m = HashMap::new();
    m.insert("inputGain1".into(), Value::from(0.0));
    m.insert("inputGain2".into(), Value::from(0.0));
    m.insert("output1volume".into(), Value::from(0.0));
    m.insert("output2volume".into(), Value::from(0.0));
    m.insert("headphoneVolume".into(), Value::from(-12.0));
    m.insert("midiPrgChChannel".into(), Value::from(0));
    m.insert("midiSnapshotPrgChChannel".into(), Value::from(0));
    m.insert("configInputMode".into(), Value::from(0));
    m.insert("configOutputMode".into(), Value::from(0));
    m.insert("expPedalMode".into(), Value::from(EXP_MODE_RING));
    m.insert("headphoneBypass".into(), Value::from(false));
    m.insert("masterVolumeChannelMode".into(), Value::from(MASTER_VOL_BOTH));
    m.insert("transportSource".into(), Value::from(SYNC_MODE_INTERNAL));
    m.insert("inputStereoLink".into(), Value::from(false));
    m.insert("outputStereoLink".into(), Value::from(false));
    m.insert("sendMidiBeatClock".into(), Value::from(false));
    m.insert("tempoBPM".into(), Value::from(120.0));
    m.insert("tempoBPB".into(), Value::from(4.0));
    m
}

fn index_to_filepath(data_dir: &Path, index: i32) -> PathBuf {
    data_dir.join(format!("profile{}.json", index))
}

pub struct Profile {
    data_dir: PathBuf,
    values: HashMap<String, Value>,
    index: i32,
    changed: bool,
}

impl Profile {
    pub fn new(settings: &Settings) -> Self {
        Self {
            data_dir: settings.data_dir.clone(),
            values: default_values(),
            index: 0,
            changed: false,
        }
    }

    pub fn get_index(&self) -> i32 {
        self.index
    }

    pub fn get_value(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    pub fn get_transport_source(&self) -> i32 {
        self.values
            .get("transportSource")
            .and_then(|v| v.as_i64())
            .unwrap_or(SYNC_MODE_INTERNAL as i64) as i32
    }

    pub fn get_tempo_bpm(&self) -> f64 {
        self.values
            .get("tempoBPM")
            .and_then(|v| v.as_f64())
            .unwrap_or(120.0)
    }

    pub fn get_tempo_bpb(&self) -> f64 {
        self.values
            .get("tempoBPB")
            .and_then(|v| v.as_f64())
            .unwrap_or(4.0)
    }

    pub fn get_midi_prgch_channel(&self, what: &str) -> i32 {
        let key = if what == "snapshot" {
            "midiSnapshotPrgChChannel"
        } else {
            "midiPrgChChannel"
        };
        self.values
            .get(key)
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32
    }

    pub fn get_exp_mode(&self) -> i32 {
        self.values
            .get("expPedalMode")
            .and_then(|v| v.as_i64())
            .unwrap_or(EXP_MODE_RING as i64) as i32
    }

    pub fn get_stereo_link(&self, port_type: &str) -> bool {
        let key = if port_type == "input" {
            "inputStereoLink"
        } else {
            "outputStereoLink"
        };
        self.values
            .get(key)
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    pub fn get_send_midi_beat_clock(&self) -> bool {
        self.values
            .get("sendMidiBeatClock")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    /// Set a value, returning true if it changed.
    pub fn set_value(&mut self, key: &str, value: Value) -> bool {
        if self.values.get(key) == Some(&value) {
            return false;
        }
        self.values.insert(key.to_string(), value);
        self.changed = true;
        true
    }

    pub fn set_tempo_bpm(&mut self, bpm: f64) -> bool {
        let clamped = bpm.clamp(20.0, 280.0);
        self.set_value("tempoBPM", Value::from(clamped))
    }

    pub fn set_tempo_bpb(&mut self, bpb: f64) -> bool {
        let clamped = bpb.clamp(1.0, 16.0);
        self.set_value("tempoBPB", Value::from(clamped))
    }

    pub fn set_sync_mode(&mut self, mode: i32) -> bool {
        self.set_value("transportSource", Value::from(mode))
    }

    pub fn set_send_midi_beat_clock(&mut self, val: bool) -> bool {
        self.set_value("sendMidiBeatClock", Value::from(val))
    }

    /// Store current profile to a file.
    pub fn store(&mut self, index: i32) {
        self.index = index;
        self.changed = false;

        let path = index_to_filepath(&self.data_dir, index);
        let json = serde_json::to_string_pretty(&self.values).unwrap_or_default();
        if let Err(e) = utils::text_file_flusher(&path, &json) {
            tracing::error!("[profile] failed to store profile {}: {}", index, e);
        }

        // Also save intermediate profile (index 5) for recovery
        let intermediate = index_to_filepath(&self.data_dir, 5);
        let mut data = self.values.clone();
        data.insert("index".into(), Value::from(index));
        let json = serde_json::to_string_pretty(&data).unwrap_or_default();
        let _ = utils::text_file_flusher(&intermediate, &json);
    }

    /// Retrieve a stored profile.
    pub fn retrieve(&mut self, index: i32) {
        let path = index_to_filepath(&self.data_dir, index);
        let loaded: HashMap<String, Value> = utils::safe_json_load(&path);

        if loaded.is_empty() {
            tracing::warn!("[profile] profile {} not found, using defaults", index);
            self.values = default_values();
        } else {
            // Merge loaded values over defaults
            let mut defaults = default_values();
            for (k, v) in loaded {
                defaults.insert(k, v);
            }
            self.values = defaults;
        }

        self.index = index;
        self.changed = false;
    }

    /// Apply mixer values to hardware via mod-amixer.
    pub fn apply_mixer_values(&self) {
        if !Path::new("/usr/bin/mod-amixer").exists() {
            return;
        }
        let soundcard = match std::env::var("MOD_SOUNDCARD").ok() {
            Some(s) if !s.is_empty() => s,
            _ => return,
        };

        let val = |key: &str| -> f64 {
            self.values.get(key).and_then(|v| v.as_f64()).unwrap_or(0.0)
        };
        let sval = |key: &str| -> String {
            self.values.get(key).and_then(|v| v.as_str())
                .unwrap_or("").to_string()
        };

        match soundcard.as_str() {
            "MODDUO" => {
                run_amixer(&format!("in 1 dvol {}", val("input1volume")));
                run_amixer(&format!("in 2 dvol {}", val("input2volume")));
                run_amixer(&format!("out 1 dvol {}", val("output1volume")));
                run_amixer(&format!("out 2 dvol {}", val("output2volume")));
                run_amixer(&format!("hp dvol {}", val("headphoneVolume")));
                run_amixer(&format!("hp byp {}", sval("headphoneBypass")));
            }
            "DUOX" => {
                run_amixer(&format!("in 1 xvol {}", val("input1volume")));
                run_amixer(&format!("in 2 xvol {}", val("input2volume")));
                run_amixer(&format!("out 1 xvol {}", val("output1volume")));
                run_amixer(&format!("out 2 xvol {}", val("output2volume")));
                run_amixer(&format!("hp xvol {}", val("headphoneVolume")));
                run_amixer(&format!("cvhp {}", sval("outputMode")));
                run_amixer(&format!("cvexp {}", sval("inputMode")));
                run_amixer(&format!("exppedal {}", sval("expPedalMode")));
            }
            "DWARF" => {
                run_amixer(&format!("in 1 xvol {}", val("input1volume")));
                run_amixer(&format!("in 2 xvol {}", val("input2volume")));
                run_amixer(&format!("out 1 xvol {}", val("output1volume")));
                run_amixer(&format!("out 2 xvol {}", val("output2volume")));
                run_amixer(&format!("hp xvol {}", val("headphoneVolume")));
            }
            _ => {
                tracing::error!("[profile] apply_mixer_values: unknown soundcard {}", soundcard);
            }
        }
    }

    /// Read current mixer values from hardware.
    pub fn fill_in_mixer_values(&mut self) {
        if !Path::new("/usr/bin/mod-amixer").exists() {
            return;
        }
        let soundcard = match std::env::var("MOD_SOUNDCARD").ok() {
            Some(s) if !s.is_empty() => s,
            _ => return,
        };

        match soundcard.as_str() {
            "MODDUO" => {
                set_from_amixer(&mut self.values, "input1volume", "in 1 dvol");
                set_from_amixer(&mut self.values, "input2volume", "in 2 dvol");
                set_from_amixer(&mut self.values, "output1volume", "out 1 dvol");
                set_from_amixer(&mut self.values, "output2volume", "out 2 dvol");
                set_from_amixer(&mut self.values, "headphoneVolume", "hp dvol");
                set_str_from_amixer(&mut self.values, "headphoneBypass", "hp byp");
            }
            "DUOX" => {
                set_from_amixer(&mut self.values, "input1volume", "in 1 xvol");
                set_from_amixer(&mut self.values, "input2volume", "in 2 xvol");
                set_from_amixer(&mut self.values, "output1volume", "out 1 xvol");
                set_from_amixer(&mut self.values, "output2volume", "out 2 xvol");
                set_from_amixer(&mut self.values, "headphoneVolume", "hp xvol");
                set_str_from_amixer(&mut self.values, "outputMode", "cvhp");
                set_str_from_amixer(&mut self.values, "inputMode", "cvexp");
                set_str_from_amixer(&mut self.values, "expPedalMode", "exppedal");
            }
            "DWARF" => {
                set_from_amixer(&mut self.values, "input1volume", "in 1 xvol");
                set_from_amixer(&mut self.values, "input2volume", "in 2 xvol");
                set_from_amixer(&mut self.values, "output1volume", "out 1 xvol");
                set_from_amixer(&mut self.values, "output2volume", "out 2 xvol");
                set_from_amixer(&mut self.values, "headphoneVolume", "hp xvol");
            }
            _ => {
                tracing::error!("[profile] fill_in_mixer_values: unknown soundcard {}", soundcard);
            }
        }
    }
}

/// Run a mod-amixer command (fire and forget).
fn run_amixer(args: &str) {
    let full = format!("/usr/bin/mod-amixer {}", args);
    if let Err(e) = std::process::Command::new("sh")
        .arg("-c")
        .arg(&full)
        .status()
    {
        tracing::error!("[profile] mod-amixer failed: {}", e);
    }
}

/// Query mod-amixer and store the float result in the values map.
fn set_from_amixer(values: &mut HashMap<String, Value>, key: &str, args: &str) {
    let full = format!("/usr/bin/mod-amixer {}", args);
    if let Ok(output) = std::process::Command::new("sh")
        .arg("-c")
        .arg(&full)
        .output()
    {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if let Ok(v) = s.parse::<f64>() {
            values.insert(key.to_string(), Value::from(v));
        }
    }
}

/// Query mod-amixer and store the string result in the values map.
fn set_str_from_amixer(values: &mut HashMap<String, Value>, key: &str, args: &str) {
    let full = format!("/usr/bin/mod-amixer {}", args);
    if let Ok(output) = std::process::Command::new("sh")
        .arg("-c")
        .arg(&full)
        .output()
    {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !s.is_empty() {
            values.insert(key.to_string(), Value::from(s));
        }
    }
}

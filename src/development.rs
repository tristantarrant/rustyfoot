// Development/testing stubs, ported from mod/development.py
// FakeHMI logs messages instead of sending them to hardware.

use crate::hmi::{Hmi, HmiCallback};
use crate::protocol::RespValue;

/// A fake HMI that logs messages and immediately calls callbacks with success values.
pub struct FakeHmi {
    initialized: std::sync::atomic::AtomicBool,
}

impl FakeHmi {
    pub fn new() -> Self {
        tracing::info!("Using FakeHMI");
        FakeHmi {
            initialized: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn respond(callback: Option<HmiCallback>, datatype: &str) {
        if let Some(cb) = callback {
            let resp = match datatype {
                "boolean" => RespValue::Bool(true),
                "string" => RespValue::Str(String::new()),
                _ => RespValue::Int(0),
            };
            cb(resp);
        }
    }
}

impl Hmi for FakeHmi {
    fn is_fake(&self) -> bool {
        true
    }

    fn send(&self, msg: &str, callback: Option<HmiCallback>, datatype: &str) {
        tracing::debug!("[fake-hmi] {}", msg);
        Self::respond(callback, datatype);
    }

    fn ping(&self, callback: HmiCallback) {
        self.send("pi", Some(callback), "boolean");
    }

    fn clear(&self, callback: HmiCallback) {
        self.send("pcl", Some(callback), "int");
    }

    fn ui_con(&self, callback: HmiCallback) {
        self.send("uc", Some(callback), "boolean");
    }

    fn ui_dis(&self, callback: HmiCallback) {
        self.send("ud", Some(callback), "boolean");
    }

    fn control_set(&self, hw_id: i32, value: f64, callback: HmiCallback) {
        self.send(
            &format!("s {} {}", hw_id, value),
            Some(callback),
            "boolean",
        );
    }

    fn control_rm(&self, hw_ids: &[i32], callback: HmiCallback) {
        let ids: Vec<String> = hw_ids.iter().map(|id| id.to_string()).collect();
        self.send(
            &format!("d {}", ids.join(" ")),
            Some(callback),
            "boolean",
        );
    }

    fn set_pedalboard_name(&self, name: &str, callback: HmiCallback) {
        self.send(&format!("pn \"{}\"", name), Some(callback), "int");
    }

    fn set_snapshot_name(&self, index: i32, name: &str, callback: HmiCallback) {
        self.send(
            &format!("sn {} \"{}\"", index, name),
            Some(callback),
            "int",
        );
    }

    fn set_profile_value(&self, key: i32, value: f64, callback: HmiCallback) {
        self.send(
            &format!("c {} {}", key, value as i64),
            Some(callback),
            "boolean",
        );
    }

    fn set_pedalboard_index(&self, index: i32, callback: HmiCallback) {
        self.send(&format!("pchng {}", index), Some(callback), "int");
    }

    fn set_available_pages(&self, pages: &[i32], callback: HmiCallback) {
        let page_str: Vec<String> = pages.iter().map(|p| p.to_string()).collect();
        self.send(
            &format!("pa {}", page_str.join(" ")),
            Some(callback),
            "boolean",
        );
    }

    fn restore(&self, callback: HmiCallback) {
        self.send("restore", Some(callback), "int");
    }

    fn tuner(&self, freq: f64, note: &str, cents: i32, callback: HmiCallback) {
        self.send(
            &format!("ts {} {} {}", freq, note, cents),
            Some(callback),
            "int",
        );
    }

    fn set_tuner_input(&self, port: i32, callback: HmiCallback) {
        self.send(&format!("ti {}", port), Some(callback), "int");
    }

    fn set_tuner_ref_freq(&self, freq: i32, callback: HmiCallback) {
        self.send(&format!("tr {}", freq), Some(callback), "int");
    }

    fn screenshot(&self, screen: i32, callback: HmiCallback) {
        self.send(
            &format!("screenshot {} ignored", screen),
            Some(callback),
            "int",
        );
    }

    fn initialized(&self) -> bool {
        self.initialized
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    fn set_initialized(&self, val: bool) {
        self.initialized
            .store(val, std::sync::atomic::Ordering::Relaxed);
    }
}

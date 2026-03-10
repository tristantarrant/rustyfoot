// Device identity, ported from mod/communication/device.py
// Reads device UID, tag, RSA keys, and image version from settings.

use std::fs;
use std::path::Path;

use crate::settings::Settings;

/// Read a value that might be a file path or a literal string.
/// If the value is a path to an existing file, read and trim its contents.
/// Otherwise return the value itself.
fn read_or_literal(value: &str) -> String {
    let p = Path::new(value);
    if p.is_file() {
        fs::read_to_string(p)
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| value.to_string())
    } else {
        value.to_string()
    }
}

pub fn get_uid(settings: &Settings) -> Option<String> {
    settings.device_uid.as_deref().map(read_or_literal)
}

pub fn get_tag(settings: &Settings) -> Option<String> {
    settings.device_tag.as_deref().map(read_or_literal)
}

pub fn get_device_key(settings: &Settings) -> Option<String> {
    settings.device_key.as_deref().map(read_or_literal)
}

pub fn get_server_key(settings: &Settings) -> Option<String> {
    settings.api_key.as_deref().map(read_or_literal)
}

pub fn get_image_version(settings: &Settings) -> String {
    settings
        .image_version
        .clone()
        .unwrap_or_else(|| "none".to_string())
}

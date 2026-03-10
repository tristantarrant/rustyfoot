// Shared utility functions, ported from mod/__init__.py

use serde_json::Value;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// Load a JSON file, returning a default value on error.
pub fn safe_json_load<T: serde::de::DeserializeOwned + Default>(path: &Path) -> T {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Load a JSON file as a serde_json::Value, returning the given default on error.
pub fn safe_json_load_value(path: &Path, default: Value) -> Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(default)
}

/// Atomically write to a file by writing then flushing (equivalent to TextFileFlusher in Python).
pub fn atomic_write(path: &Path, contents: &str) -> io::Result<()> {
    let mut f = fs::File::create(path)?;
    f.write_all(contents.as_bytes())?;
    f.flush()?;
    f.sync_all()?;
    Ok(())
}

/// Escape backslashes and single quotes, then squeeze whitespace (like mod_squeeze in Python).
pub fn mod_squeeze(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('\'', "\\'");
    squeeze(&escaped)
}

/// Collapse runs of whitespace into single spaces and trim.
fn squeeze(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut prev_ws = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !prev_ws {
                result.push(' ');
            }
            prev_ws = true;
        } else {
            result.push(ch);
            prev_ws = false;
        }
    }
    result
}

/// Get hardware descriptor from JSON file.
pub fn get_hardware_descriptor(path: &Path) -> serde_json::Map<String, Value> {
    safe_json_load_value(path, Value::Object(Default::default()))
        .as_object()
        .cloned()
        .unwrap_or_default()
}

/// Make a string safe for use as a symbol (alphanumeric + underscore).
pub fn symbolify(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}

/// Sync filesystem (like os.sync() in Python).
pub fn os_sync() {
    unsafe {
        libc::sync();
    }
}

/// Atomically write via a temp file then rename (matches Python's TextFileFlusher).
pub fn text_file_flusher(path: &Path, contents: &str) -> io::Result<()> {
    let tmp_path = path.with_extension("tmp");
    let mut f = fs::File::create(&tmp_path)?;
    f.write_all(contents.as_bytes())?;
    f.flush()?;
    f.sync_all()?;
    drop(f);
    fs::rename(&tmp_path, path)?;
    Ok(())
}

/// Ensure required directories and files exist. Ported from check_environment() in mod/__init__.py.
pub fn check_environment(settings: &crate::settings::Settings) -> bool {
    // Create temp dirs
    if !settings.download_tmp_dir.exists() {
        if let Err(e) = fs::create_dir_all(&settings.download_tmp_dir) {
            tracing::error!("Cannot create download tmp dir: {}", e);
        }
    }

    if settings.pedalboard_tmp_dir.exists() {
        let _ = fs::remove_dir_all(&settings.pedalboard_tmp_dir);
    }
    if let Err(e) = fs::create_dir_all(&settings.pedalboard_tmp_dir) {
        tracing::error!("Cannot create pedalboard tmp dir: {}", e);
    }

    // Remove temp files
    for path in [
        &settings.capture_path,
        &settings.playback_path,
        &settings.update_cc_firmware_file,
    ] {
        if path.exists() {
            let _ = fs::remove_file(path);
        }
    }

    // Check RW access to data dir
    if settings.data_dir.exists() {
        if fs::metadata(&settings.data_dir)
            .map(|m| m.permissions().readonly())
            .unwrap_or(true)
        {
            tracing::error!("No write access to data dir {:?}", settings.data_dir);
            return false;
        }
    } else if let Err(e) = fs::create_dir_all(&settings.data_dir) {
        tracing::error!("Cannot create data dir {:?}: {}", settings.data_dir, e);
        return false;
    }

    // Create needed dirs
    let keys_path = Path::new(&settings.keys_path);
    if !keys_path.exists() {
        let _ = fs::create_dir_all(keys_path);
    }

    if !settings.lv2_pedalboards_dir.exists() {
        let _ = fs::create_dir_all(&settings.lv2_pedalboards_dir);
    }

    // Create default JSON files if missing
    if !settings.user_banks_json_file.exists() {
        let _ = fs::write(&settings.user_banks_json_file, "[]");
    }
    if !settings.favorites_json_file.exists() {
        let _ = fs::write(&settings.favorites_json_file, "[]");
    }

    // Clean up old update files
    if settings.update_mod_os_file.exists() && !Path::new("/root/check-upgrade-system").exists() {
        let _ = fs::remove_file(&settings.update_mod_os_file);
        os_sync();
    }
    if settings.update_mod_os_helper_file.exists() {
        let _ = fs::remove_file(&settings.update_mod_os_helper_file);
        os_sync();
    }

    true
}

/// Normalize a string for hardware display (ASCII, uppercase, quoted, limited length).
pub fn normalize_for_hw(s: &str, limit: usize) -> String {
    let ascii: String = s
        .chars()
        .filter(|c| c.is_ascii() && *c != '"')
        .take(limit)
        .collect();
    format!("\"{}\"", ascii.to_uppercase())
}

/// Find the nearest valid scalepoint value from a list of (value, label) options.
pub fn get_nearest_valid_scalepoint_value(
    value: f64,
    options: &[(f64, String)],
) -> Option<(usize, f64)> {
    if options.is_empty() {
        return None;
    }

    // Exact match
    for (i, (ovalue, _)) in options.iter().enumerate() {
        if *ovalue == value {
            return Some((i, *ovalue));
        }
    }

    // Near match
    for (i, (ovalue, _)) in options.iter().enumerate() {
        if (ovalue - value).abs() <= 0.0001 {
            return Some((i, *ovalue));
        }
    }

    // Closest match
    let mut smallest_diff = f64::MAX;
    let mut smallest_pos = 0;
    for (i, (ovalue, _)) in options.iter().enumerate() {
        let diff = (ovalue - value).abs();
        if diff < smallest_diff {
            smallest_diff = diff;
            smallest_pos = i;
        }
    }

    Some((smallest_pos, options[smallest_pos].0))
}

// Bank management, ported from mod/bank.py

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use tracing;

use crate::utils;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pedalboard {
    pub title: String,
    #[serde(default)]
    pub bundle: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bank {
    pub title: String,
    pub pedalboards: Vec<Pedalboard>,
}

/// Return list of banks from a JSON file, validating pedalboard references.
pub fn list_banks(
    banks_file: &Path,
    broken_pedal_bundles: &[String],
    user_banks: bool,
    should_save: bool,
) -> Vec<Bank> {
    let mut banks: Vec<Bank> = utils::safe_json_load(banks_file);

    if banks.is_empty() {
        return Vec::new();
    }

    let mut changed = false;
    let check_broken = !broken_pedal_bundles.is_empty();
    let mut used_names: Vec<String> = Vec::new();

    for bank in &mut banks {
        // Ensure unique bank names for user banks
        if user_banks {
            if let Some(new_title) = get_unique_name(&bank.title, &used_names) {
                bank.title = new_title;
                changed = true;
            }
            used_names.push(bank.title.clone());
        }

        // Validate pedalboards exist
        let mut valid_pedals = Vec::new();

        for pb in &bank.pedalboards {
            if pb.bundle.is_empty() {
                let title: String = pb.title.chars().filter(|c| c.is_ascii()).collect();
                tracing::warn!(
                    "Auto-removing pedalboard '{}' from bank (missing bundle)",
                    title
                );
                changed = true;
                continue;
            }

            if !Path::new(&pb.bundle).exists() {
                let bundle: String = pb.bundle.chars().filter(|c| c.is_ascii()).collect();
                tracing::error!(
                    "Referenced pedalboard does not exist: {}",
                    bundle
                );
                changed = true;
                continue;
            }

            if check_broken {
                let abs = std::fs::canonicalize(&pb.bundle)
                    .unwrap_or_else(|_| Path::new(&pb.bundle).to_path_buf());
                if broken_pedal_bundles
                    .iter()
                    .any(|b| Path::new(b) == abs)
                {
                    let title: String = pb.title.chars().filter(|c| c.is_ascii()).collect();
                    tracing::warn!(
                        "Auto-removing pedalboard '{}' from bank (it's broken)",
                        title
                    );
                    changed = true;
                    continue;
                }
            }

            valid_pedals.push(pb.clone());
        }

        if valid_pedals.is_empty() {
            let title: String = bank.title.chars().filter(|c| c.is_ascii()).collect();
            tracing::debug!(
                "Bank '{}' does not contain any pedalboards",
                title
            );
        }

        bank.pedalboards = valid_pedals;
    }

    if user_banks && changed && should_save {
        save_banks(banks_file, &banks);
    }

    banks
}

/// Save user banks to disk.
pub fn save_banks(banks_file: &Path, banks: &[Bank]) {
    let json = serde_json::to_string_pretty(banks).unwrap_or_else(|_| "[]".to_string());
    if let Err(e) = utils::atomic_write(banks_file, &json) {
        tracing::error!("Failed to save banks: {}", e);
    }
}

/// Save last selected bank ID and pedalboard path to disk.
pub fn save_last_bank_and_pedalboard(
    last_state_file: &Path,
    bank: Option<i32>,
    pedalboard: &str,
) {
    let bank = match bank {
        Some(b) => b,
        None => return,
    };

    let data = serde_json::json!({
        "bank": bank - 2,
        "pedalboard": pedalboard,
        "supportsDividers": true,
    });

    let json = serde_json::to_string(&data).unwrap_or_default();
    if let Err(e) = utils::atomic_write(last_state_file, &json) {
        tracing::error!("Failed to save last state: {}", e);
    }
}

/// Get last bank index and pedalboard path from last.json.
/// Returns (bank_index, pedalboard_path) where bank_index is the raw value from the file
/// (-1 = "All Pedalboards", 0 = first user bank, 1 = second user bank, etc.).
/// The caller must add `userbanks_offset` to convert to a `bank_id`.
pub fn get_last_bank_and_pedalboard(last_state_file: &Path) -> (i32, Option<String>) {
    let data: Value = utils::safe_json_load_value(last_state_file, Value::Object(Default::default()));

    let obj = match data.as_object() {
        Some(o) => o,
        None => {
            tracing::warn!("last state file does not exist or is corrupt");
            return (-1, None);
        }
    };

    let bank = match obj.get("bank").and_then(|v| v.as_i64()) {
        Some(b) => b as i32,
        None => {
            tracing::warn!("last state file does not exist or is corrupt");
            return (-1, None);
        }
    };

    let pedalboard = obj.get("pedalboard").and_then(|v| v.as_str()).map(|s| s.to_string());

    (bank, pedalboard)
}

/// Remove a pedalboard from all user banks.
pub fn remove_pedalboard_from_banks(banks_file: &Path, pedalboard: &str) {
    let mut banks: Vec<Bank> = utils::safe_json_load(banks_file);

    let target = std::fs::canonicalize(pedalboard)
        .unwrap_or_else(|_| Path::new(pedalboard).to_path_buf());

    for bank in &mut banks {
        bank.pedalboards.retain(|pb| {
            let pb_path = std::fs::canonicalize(&pb.bundle)
                .unwrap_or_else(|_| Path::new(&pb.bundle).to_path_buf());
            pb_path != target
        });

        if bank.pedalboards.is_empty() {
            let title: String = bank.title.chars().filter(|c| c.is_ascii()).collect();
            tracing::debug!(
                "Bank '{}' does not contain any pedalboards",
                title
            );
        }
    }

    save_banks(banks_file, &banks);
}

/// Generate a unique name by appending " (N)" if the name already exists.
fn get_unique_name(name: &str, names: &[String]) -> Option<String> {
    if !names.contains(&name.to_string()) {
        return None;
    }

    let re = regex::Regex::new(r"^.* \(([0-9]+)\)$").unwrap();

    let mut candidate = if re.is_match(name) {
        name.to_string()
    } else {
        format!("{} (2)", name)
    };

    loop {
        if !names.contains(&candidate) {
            return Some(candidate);
        }
        if let Some(caps) = re.captures(&candidate) {
            let num: u32 = caps[1].parse().unwrap_or(1);
            let prefix = &candidate[..candidate.rfind('(').unwrap()];
            candidate = format!("{}({})", prefix, num + 1);
        } else {
            break;
        }
    }

    Some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_unique_name_no_conflict() {
        let names = vec!["Bank A".to_string(), "Bank B".to_string()];
        assert_eq!(get_unique_name("Bank C", &names), None);
    }

    #[test]
    fn test_get_unique_name_with_conflict() {
        let names = vec!["Bank A".to_string()];
        assert_eq!(
            get_unique_name("Bank A", &names),
            Some("Bank A (2)".to_string())
        );
    }

    #[test]
    fn test_get_unique_name_multiple_conflicts() {
        let names = vec![
            "Bank A".to_string(),
            "Bank A (2)".to_string(),
            "Bank A (3)".to_string(),
        ];
        assert_eq!(
            get_unique_name("Bank A", &names),
            Some("Bank A (4)".to_string())
        );
    }
}

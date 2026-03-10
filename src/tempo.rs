// Tempo subdivision utilities, ported from modtools/tempo.py

/// A tempo subdivision divider with its value and label.
#[derive(Debug, Clone)]
pub struct Divider {
    pub value: f64,
    pub label: &'static str,
}

/// All available tempo subdivisions.
pub const DIVIDERS: &[Divider] = &[
    Divider { value: 0.333, label: "2." },
    Divider { value: 0.5, label: "2" },
    Divider { value: 0.75, label: "2T" },
    Divider { value: 0.666, label: "1." },
    Divider { value: 1.0, label: "1" },
    Divider { value: 1.5, label: "1T" },
    Divider { value: 1.333, label: "1/2." },
    Divider { value: 2.0, label: "1/2" },
    Divider { value: 3.0, label: "1/2T" },
    Divider { value: 2.666, label: "1/4." },
    Divider { value: 4.0, label: "1/4" },
    Divider { value: 6.0, label: "1/4T" },
    Divider { value: 5.333, label: "1/8." },
    Divider { value: 8.0, label: "1/8" },
    Divider { value: 12.0, label: "1/8T" },
    Divider { value: 10.666, label: "1/16." },
    Divider { value: 16.0, label: "1/16" },
    Divider { value: 24.0, label: "1/16T" },
    Divider { value: 21.333, label: "1/32." },
    Divider { value: 32.0, label: "1/32" },
    Divider { value: 48.0, label: "1/32T" },
];

/// Unit conversion factors for converting to/from seconds and Hz.
pub struct UnitConversion {
    pub to: f64,
    pub from: f64,
}

/// Get conversion factors for a given unit symbol. Returns None for unknown units.
pub fn unit_conversion(unit: &str) -> Option<UnitConversion> {
    match unit {
        "s" => Some(UnitConversion { to: 1.0, from: 1.0 }),
        "ms" => Some(UnitConversion { to: 0.001, from: 1000.0 }),
        "min" => Some(UnitConversion { to: 60.0, from: 1.0 / 60.0 }),
        "Hz" => Some(UnitConversion { to: 1.0, from: 1.0 }),
        "MHz" => Some(UnitConversion { to: 1_000_000.0, from: 0.000_001 }),
        "kHz" => Some(UnitConversion { to: 1000.0, from: 0.001 }),
        _ => None,
    }
}

/// Get filtered dividers where smin <= value <= smax.
pub fn get_filtered_dividers(smin: f64, smax: f64) -> Vec<&'static Divider> {
    DIVIDERS
        .iter()
        .filter(|d| d.value >= smin && d.value <= smax)
        .collect()
}

/// Compute divider value: 240 / (bpm * port_value_seconds).
pub fn get_divider_value(bpm: f64, value_seconds: f64) -> f64 {
    240.0 / (bpm * value_seconds)
}

/// Compute control port value given BPM and subdivider.
pub fn get_port_value(bpm: f64, subdivider: f64, port_unit_symbol: &str) -> f64 {
    if port_unit_symbol == "BPM" {
        bpm / subdivider
    } else {
        240.0 / (bpm * subdivider)
    }
}

/// Convert value between units using a conversion factor.
/// For time-like units (s, ms, min): factor * value
/// For frequency-like units (Hz, kHz, MHz): factor / value
fn convert_equivalent(value: f64, conversion_factor: f64, port_unit_symbol: &str) -> Option<f64> {
    let value = if value == 0.0 { 0.001 } else { value };
    match port_unit_symbol {
        "s" | "ms" | "min" => Some(conversion_factor * value),
        "Hz" | "MHz" | "kHz" => Some(conversion_factor / value),
        _ => None,
    }
}

/// Convert a value in seconds to the equivalent in the given port unit.
pub fn convert_seconds_to_port_value(value: f64, port_unit_symbol: &str) -> Option<f64> {
    let conv = unit_conversion(port_unit_symbol)?;
    convert_equivalent(value, conv.from, port_unit_symbol)
}

/// Convert a value from the given port unit to the equivalent in seconds.
pub fn convert_port_value_to_seconds(value: f64, port_unit_symbol: &str) -> Option<f64> {
    let conv = unit_conversion(port_unit_symbol)?;
    convert_equivalent(value, conv.to, port_unit_symbol)
}

/// Port range and unit info, matching the structure from LV2 plugin data.
pub struct PortInfo {
    pub min: f64,
    pub max: f64,
    pub unit_symbol: String,
    pub has_strict_bounds: bool,
}

/// Get available divider options for a port given BPM range.
pub fn get_divider_options(port: &PortInfo, min_bpm: f64, max_bpm: f64) -> Vec<&'static Divider> {
    let (s1_min, s2_min, s1_max, s2_max) = if port.unit_symbol == "BPM" {
        (
            min_bpm / port.min,
            min_bpm / port.max,
            max_bpm / port.min,
            max_bpm / port.max,
        )
    } else {
        let min_sec = convert_port_value_to_seconds(port.min, &port.unit_symbol).unwrap_or(port.min);
        let max_sec = convert_port_value_to_seconds(port.max, &port.unit_symbol).unwrap_or(port.max);
        (
            get_divider_value(min_bpm, min_sec),
            get_divider_value(min_bpm, max_sec),
            get_divider_value(max_bpm, min_sec),
            get_divider_value(max_bpm, max_sec),
        )
    };

    let (smin, smax) = if port.has_strict_bounds {
        if s1_min < s2_min {
            (s1_min.max(s1_max), s2_min.min(s2_max))
        } else {
            (s2_min.max(s2_max), s1_min.min(s1_max))
        }
    } else {
        let all = [s1_min, s2_min, s1_max, s2_max];
        (
            all.iter().cloned().fold(f64::INFINITY, f64::min),
            all.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        )
    };

    get_filtered_dividers(smin, smax)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dividers_count() {
        assert_eq!(DIVIDERS.len(), 21);
    }

    #[test]
    fn test_filtered_dividers() {
        let filtered = get_filtered_dividers(1.0, 4.0);
        assert!(filtered.iter().any(|d| d.label == "1"));
        assert!(filtered.iter().any(|d| d.label == "1/4"));
        assert!(!filtered.iter().any(|d| d.label == "1/8"));
    }

    #[test]
    fn test_get_port_value_bpm() {
        let val = get_port_value(120.0, 4.0, "BPM");
        assert!((val - 30.0).abs() < 0.001);
    }

    #[test]
    fn test_get_port_value_seconds() {
        let val = get_port_value(120.0, 4.0, "s");
        assert!((val - 0.5).abs() < 0.001); // 240 / (120 * 4) = 0.5
    }

    #[test]
    fn test_unit_conversion_ms() {
        let val = convert_seconds_to_port_value(0.5, "ms").unwrap();
        assert!((val - 500.0).abs() < 0.001);

        let val = convert_port_value_to_seconds(500.0, "ms").unwrap();
        assert!((val - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_unit_conversion_hz() {
        let val = convert_seconds_to_port_value(0.5, "Hz").unwrap();
        assert!((val - 2.0).abs() < 0.001); // 1.0 / 0.5

        let val = convert_port_value_to_seconds(2.0, "Hz").unwrap();
        assert!((val - 0.5).abs() < 0.001); // 1.0 / 2.0
    }
}

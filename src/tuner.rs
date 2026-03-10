// Musical tuner utility
// Ported from mod/tuner.py

const NOTES: [&str; 12] = [
    "A", "A#", "B", "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#",
];

/// Convert a frequency to (frequency, note_name, cents_offset).
/// Returns None if frequency is <= 0.
pub fn find_freq_note_cents(freq: f64) -> Option<(f64, String, i32)> {
    if freq <= 0.0 {
        return None;
    }

    // Number of half-steps from A4 (440 Hz)
    let half_steps = 12.0 * (freq / 440.0).log2();
    let nearest = half_steps.round() as i32;
    let cents = ((half_steps - nearest as f64) * 100.0).round() as i32;

    // Note index (A=0, A#=1, ..., G#=11)
    let note_idx = ((nearest % 12) + 12) % 12;
    let octave = 4 + (nearest + 9) / 12; // A4 is in octave 4

    let note_name = format!("{}{}", NOTES[note_idx as usize], octave);
    Some((freq, note_name, cents))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_a440() {
        let (_, note, cents) = find_freq_note_cents(440.0).unwrap();
        assert_eq!(note, "A4");
        assert_eq!(cents, 0);
    }

    #[test]
    fn test_middle_c() {
        let (_, note, cents) = find_freq_note_cents(261.63).unwrap();
        assert_eq!(note, "C4");
        assert!(cents.abs() <= 1);
    }
}

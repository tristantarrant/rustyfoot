// Transport state management (BPM, BPB, sync, rolling).
// Ported from transport-related parts of mod/host.py

/// Transport sync mode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SyncMode {
    Internal,
    MidiSlave,
    AbletonLink,
}

impl SyncMode {
    pub fn from_str(s: &str) -> Self {
        match s {
            "midi_clock_slave" => Self::MidiSlave,
            "link" => Self::AbletonLink,
            _ => Self::Internal,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Internal => "none",
            Self::MidiSlave => "midi_clock_slave",
            Self::AbletonLink => "link",
        }
    }
}

/// Transport state for the audio engine.
#[derive(Debug, Clone)]
pub struct TransportState {
    pub rolling: bool,
    pub bpb: f64,
    pub bpm: f64,
    pub sync: SyncMode,
}

impl Default for TransportState {
    fn default() -> Self {
        Self {
            rolling: false,
            bpb: 4.0,
            bpm: 120.0,
            sync: SyncMode::Internal,
        }
    }
}

impl TransportState {
    /// Format as mod-host transport command arguments.
    pub fn as_command_args(&self) -> String {
        format!(
            "{} {} {}",
            if self.rolling { 1 } else { 0 },
            self.bpb,
            self.bpm
        )
    }
}

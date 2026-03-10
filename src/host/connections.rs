// Port connection management and JACK port tracking.
// Ported from connection-related parts of mod/host.py

/// A connection between two ports in the audio graph.
#[derive(Debug, Clone, PartialEq)]
pub struct Connection {
    pub port_from: String,
    pub port_to: String,
}

/// MIDI port info.
#[derive(Debug, Clone)]
pub struct MidiPort {
    pub symbol: String,
    pub alias: String,
    pub pending_connections: Vec<String>,
}

/// Tracks all port connections and external JACK ports.
#[derive(Debug, Clone, Default)]
pub struct ConnectionManager {
    pub connections: Vec<Connection>,
    pub audio_ports_in: Vec<String>,
    pub audio_ports_out: Vec<String>,
    pub cv_ports_in: Vec<String>,
    pub cv_ports_out: Vec<String>,
    pub midi_ports: Vec<MidiPort>,
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.connections.clear();
        // Audio/CV/MIDI ports are not cleared (they represent hardware)
    }

    /// Add a connection.
    pub fn add(&mut self, port_from: &str, port_to: &str) {
        let conn = Connection {
            port_from: port_from.to_string(),
            port_to: port_to.to_string(),
        };
        if !self.connections.contains(&conn) {
            self.connections.push(conn);
        }
    }

    /// Remove a connection.
    pub fn remove(&mut self, port_from: &str, port_to: &str) {
        self.connections
            .retain(|c| !(c.port_from == port_from && c.port_to == port_to));
    }

    /// Remove all connections involving a given port prefix (e.g., when removing a plugin).
    pub fn remove_by_prefix(&mut self, prefix: &str) {
        self.connections
            .retain(|c| !c.port_from.starts_with(prefix) && !c.port_to.starts_with(prefix));
    }

    /// Get all connections as (from, to) pairs.
    pub fn get_all(&self) -> &[Connection] {
        &self.connections
    }

    /// Check if a specific connection exists.
    pub fn has_connection(&self, port_from: &str, port_to: &str) -> bool {
        self.connections
            .iter()
            .any(|c| c.port_from == port_from && c.port_to == port_to)
    }

    /// Handle JACK port appeared.
    pub fn jack_port_appeared(&mut self, name: &str, is_output: bool) {
        // Categorize by name prefix
        if name.starts_with("system:capture_") || name.contains(":out") {
            if !is_output {
                return;
            }
            if !self.audio_ports_in.contains(&name.to_string()) {
                self.audio_ports_in.push(name.to_string());
            }
        } else if name.starts_with("system:playback_") || name.contains(":in") {
            if is_output {
                return;
            }
            if !self.audio_ports_out.contains(&name.to_string()) {
                self.audio_ports_out.push(name.to_string());
            }
        }
    }

    /// Handle JACK port deleted.
    pub fn jack_port_deleted(&mut self, name: &str) {
        self.audio_ports_in.retain(|p| p != name);
        self.audio_ports_out.retain(|p| p != name);
        self.cv_ports_in.retain(|p| p != name);
        self.cv_ports_out.retain(|p| p != name);

        // Remove any connections involving this port
        self.connections
            .retain(|c| c.port_from != name && c.port_to != name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_remove_connection() {
        let mut cm = ConnectionManager::new();
        cm.add("plugin_0:out", "plugin_1:in");
        assert!(cm.has_connection("plugin_0:out", "plugin_1:in"));
        assert_eq!(cm.connections.len(), 1);

        // No duplicates
        cm.add("plugin_0:out", "plugin_1:in");
        assert_eq!(cm.connections.len(), 1);

        cm.remove("plugin_0:out", "plugin_1:in");
        assert!(!cm.has_connection("plugin_0:out", "plugin_1:in"));
    }

    #[test]
    fn test_remove_by_prefix() {
        let mut cm = ConnectionManager::new();
        cm.add("plugin_0:out_l", "plugin_1:in_l");
        cm.add("plugin_0:out_r", "plugin_1:in_r");
        cm.add("plugin_2:out", "plugin_1:in_l");

        cm.remove_by_prefix("plugin_0");
        assert_eq!(cm.connections.len(), 1);
        assert!(cm.has_connection("plugin_2:out", "plugin_1:in_l"));
    }
}

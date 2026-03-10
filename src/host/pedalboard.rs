// Pedalboard state and snapshot management.
// Ported from pedalboard/snapshot parts of mod/host.py

use std::collections::HashMap;
use std::path::PathBuf;

/// HMI snapshot offset constants (negative IDs reserved for HMI snapshots).
pub const HMI_SNAPSHOTS_OFFSET: i32 = 100;
pub const HMI_SNAPSHOT_1: i32 = -(HMI_SNAPSHOTS_OFFSET);
pub const HMI_SNAPSHOT_2: i32 = -(HMI_SNAPSHOTS_OFFSET + 1);
pub const HMI_SNAPSHOT_3: i32 = -(HMI_SNAPSHOTS_OFFSET + 2);

/// A single pedalboard snapshot (saved parameter state).
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub name: String,
    /// Plugin instance_id -> (port_symbol -> value)
    pub data: HashMap<i32, HashMap<String, f64>>,
    /// Plugin instances added after this snapshot was created
    pub plugins_added: Vec<i32>,
}

/// Current pedalboard state.
#[derive(Debug, Clone)]
pub struct PedalboardState {
    pub name: String,
    pub path: PathBuf,
    pub modified: bool,
    pub empty: bool,
    pub size: (i32, i32),
    pub version: i32,
    pub current_snapshot_id: i32,
    pub snapshots: Vec<Option<Snapshot>>,
    pub hmi_snapshots: [Option<Snapshot>; 3],
}

impl Default for PedalboardState {
    fn default() -> Self {
        Self {
            name: String::new(),
            path: PathBuf::new(),
            modified: false,
            empty: true,
            size: (0, 0),
            version: 0,
            current_snapshot_id: -1,
            snapshots: Vec::new(),
            hmi_snapshots: [None, None, None],
        }
    }
}

impl PedalboardState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset to empty state.
    pub fn reset(&mut self) {
        self.name.clear();
        self.path = PathBuf::new();
        self.modified = false;
        self.empty = true;
        self.size = (0, 0);
        self.version = 0;
        self.current_snapshot_id = -1;
        self.snapshots.clear();
        self.hmi_snapshots = [None, None, None];
    }

    /// Get the name of the current snapshot.
    pub fn snapshot_name(&self) -> Option<&str> {
        if self.current_snapshot_id < 0 {
            return None;
        }
        self.snapshots
            .get(self.current_snapshot_id as usize)
            .and_then(|s| s.as_ref())
            .map(|s| s.name.as_str())
    }

    /// Set pedalboard position.
    pub fn set_size(&mut self, width: i32, height: i32) {
        self.size = (width, height);
        self.modified = true;
    }

    /// Create a new snapshot from current plugin states.
    pub fn snapshot_make(
        &mut self,
        name: &str,
        plugin_ports: &HashMap<i32, HashMap<String, f64>>,
    ) -> i32 {
        let snapshot = Snapshot {
            name: name.to_string(),
            data: plugin_ports.clone(),
            plugins_added: Vec::new(),
        };

        // Find first empty slot or add new
        let id = self
            .snapshots
            .iter()
            .position(|s| s.is_none())
            .unwrap_or_else(|| {
                self.snapshots.push(None);
                self.snapshots.len() - 1
            });

        self.snapshots[id] = Some(snapshot);
        self.modified = true;
        id as i32
    }

    /// Delete a snapshot by ID.
    pub fn snapshot_delete(&mut self, snapshot_id: i32) -> bool {
        let idx = snapshot_id as usize;
        if idx >= self.snapshots.len() {
            return false;
        }
        self.snapshots[idx] = None;
        self.modified = true;

        if self.current_snapshot_id == snapshot_id {
            self.current_snapshot_id = -1;
        }
        true
    }

    /// Rename a snapshot.
    pub fn snapshot_rename(&mut self, snapshot_id: i32, new_name: &str) -> bool {
        let idx = snapshot_id as usize;
        if let Some(Some(snapshot)) = self.snapshots.get_mut(idx) {
            snapshot.name = new_name.to_string();
            self.modified = true;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_lifecycle() {
        let mut pb = PedalboardState::new();
        let ports = HashMap::new();

        let id = pb.snapshot_make("Snapshot 1", &ports);
        assert_eq!(id, 0);
        assert_eq!(pb.snapshots.len(), 1);

        pb.current_snapshot_id = id;
        assert_eq!(pb.snapshot_name(), Some("Snapshot 1"));

        pb.snapshot_rename(id, "My Snapshot");
        assert_eq!(pb.snapshot_name(), Some("My Snapshot"));

        pb.snapshot_delete(id);
        assert!(pb.snapshots[0].is_none());
        assert_eq!(pb.current_snapshot_id, -1);
    }

    #[test]
    fn test_reset() {
        let mut pb = PedalboardState::new();
        pb.name = "Test".into();
        pb.modified = true;
        pb.reset();
        assert!(pb.name.is_empty());
        assert!(!pb.modified);
        assert!(pb.empty);
    }
}

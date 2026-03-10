// InstanceIdMapper: bidirectional mapping between numeric IDs and string instance names.
// Ported from InstanceIdMapper class in mod/host.py

use std::collections::HashMap;

use crate::settings::{PEDALBOARD_INSTANCE, PEDALBOARD_INSTANCE_ID};

pub struct InstanceIdMapper {
    last_id: i32,
    id_map: HashMap<i32, String>,
    instance_map: HashMap<String, i32>,
}

impl InstanceIdMapper {
    pub fn new() -> Self {
        let mut m = Self {
            last_id: 0,
            id_map: HashMap::new(),
            instance_map: HashMap::new(),
        };
        m.id_map
            .insert(PEDALBOARD_INSTANCE_ID, PEDALBOARD_INSTANCE.to_string());
        m.instance_map
            .insert(PEDALBOARD_INSTANCE.to_string(), PEDALBOARD_INSTANCE_ID);
        m
    }

    pub fn clear(&mut self) {
        self.last_id = 0;
        self.id_map.clear();
        self.instance_map.clear();
        self.id_map
            .insert(PEDALBOARD_INSTANCE_ID, PEDALBOARD_INSTANCE.to_string());
        self.instance_map
            .insert(PEDALBOARD_INSTANCE.to_string(), PEDALBOARD_INSTANCE_ID);
    }

    /// Get a numeric ID from a string instance, creating one if needed.
    pub fn get_id(&mut self, instance: &str) -> i32 {
        if let Some(&id) = self.instance_map.get(instance) {
            return id;
        }

        let idx = self.last_id;
        self.last_id += 1;

        self.instance_map.insert(instance.to_string(), idx);
        self.id_map.insert(idx, instance.to_string());

        idx
    }

    /// Get a numeric ID, trying to use a pre-defined number first.
    pub fn get_id_by_number(&mut self, instance: &str, instance_number: i32) -> i32 {
        if instance_number < 0 {
            return self.get_id(instance);
        }

        if self.id_map.contains_key(&instance_number) {
            return self.get_id(instance);
        }

        self.last_id = self.last_id.max(instance_number + 1);
        self.instance_map
            .insert(instance.to_string(), instance_number);
        self.id_map
            .insert(instance_number, instance.to_string());

        instance_number
    }

    /// Get a numeric ID without creating a new mapping. Returns None if not found.
    pub fn get_id_without_creating(&self, instance: &str) -> Option<i32> {
        self.instance_map.get(instance).copied()
    }

    /// Get a string instance from a numeric ID.
    pub fn get_instance(&self, id: i32) -> Option<&str> {
        self.id_map.get(&id).map(|s| s.as_str())
    }

    /// Get the id_map for serialization (instance_id -> instance string).
    pub fn get_id_map(&self) -> &HashMap<i32, String> {
        &self.id_map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_id_auto_increment() {
        let mut m = InstanceIdMapper::new();
        let id1 = m.get_id("/graph/plugin_0");
        let id2 = m.get_id("/graph/plugin_1");
        assert_ne!(id1, id2);
        // Same instance returns same ID
        assert_eq!(m.get_id("/graph/plugin_0"), id1);
    }

    #[test]
    fn test_get_id_by_number() {
        let mut m = InstanceIdMapper::new();
        let id = m.get_id_by_number("/graph/plugin_5", 5);
        assert_eq!(id, 5);
        assert_eq!(m.get_instance(5), Some("/graph/plugin_5"));
    }

    #[test]
    fn test_pedalboard_instance() {
        let m = InstanceIdMapper::new();
        assert_eq!(
            m.get_id_without_creating(PEDALBOARD_INSTANCE),
            Some(PEDALBOARD_INSTANCE_ID)
        );
        assert_eq!(
            m.get_instance(PEDALBOARD_INSTANCE_ID),
            Some(PEDALBOARD_INSTANCE)
        );
    }

    #[test]
    fn test_clear() {
        let mut m = InstanceIdMapper::new();
        m.get_id("/graph/plugin_0");
        m.clear();
        assert_eq!(m.get_id_without_creating("/graph/plugin_0"), None);
        // Pedalboard instance should still exist
        assert_eq!(
            m.get_id_without_creating(PEDALBOARD_INSTANCE),
            Some(PEDALBOARD_INSTANCE_ID)
        );
    }
}

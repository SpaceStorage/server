//! In-memory restored content after WAL replay (013 T061).

use crate::mode::StorageMode;
use std::collections::HashMap;
use uuid::Uuid;

/// Why a restored container is present but not readable (013 / constitution XIII).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnavailableReason {
    KeyUnresolvable,
    DecryptFailed,
}

/// Per-container KV after boot restore. Memory-mode containers stay empty.
#[derive(Debug, Default, Clone)]
pub struct ContentStore {
    /// Persistent + hybrid durable image (from WAL / SSTables).
    rows: HashMap<Uuid, HashMap<Vec<u8>, Vec<u8>>>,
    /// Hybrid memory portion rebuilt per HybridPolicy (WAL replay stub).
    hybrid_hot: HashMap<Uuid, HashMap<Vec<u8>, Vec<u8>>>,
    modes: HashMap<Uuid, StorageMode>,
    /// Encrypted containers whose data key is missing/unreadable (no ciphertext as truth).
    unavailable: HashMap<Uuid, UnavailableReason>,
}

impl ContentStore {
    pub fn register_mode(&mut self, container_id: Uuid, mode: StorageMode) {
        self.modes.insert(container_id, mode);
        match mode {
            StorageMode::Memory => {
                // Definition restored elsewhere; content stays empty.
                self.rows.entry(container_id).or_default().clear();
                self.hybrid_hot.remove(&container_id);
            }
            StorageMode::Persistent | StorageMode::Hybrid => {
                self.rows.entry(container_id).or_default();
                if matches!(mode, StorageMode::Hybrid) {
                    self.hybrid_hot.entry(container_id).or_default();
                }
            }
        }
    }

    pub fn mode(&self, container_id: Uuid) -> StorageMode {
        self.modes
            .get(&container_id)
            .copied()
            .unwrap_or(StorageMode::Persistent)
    }

    /// Mark container `Unavailable` (definition retained; content not applied).
    pub fn mark_unavailable(&mut self, container_id: Uuid, reason: UnavailableReason) {
        self.unavailable.insert(container_id, reason);
        self.rows.insert(container_id, HashMap::new());
        self.hybrid_hot.remove(&container_id);
    }

    pub fn is_unavailable(&self, container_id: Uuid) -> bool {
        self.unavailable.contains_key(&container_id)
    }

    pub fn unavailable_reason(&self, container_id: Uuid) -> Option<&UnavailableReason> {
        self.unavailable.get(&container_id)
    }

    pub fn put(&mut self, container_id: Uuid, key: Vec<u8>, value: Vec<u8>) {
        if self.is_unavailable(container_id) {
            return;
        }
        match self.mode(container_id) {
            StorageMode::Memory => {}
            StorageMode::Persistent => {
                self.rows
                    .entry(container_id)
                    .or_default()
                    .insert(key, value);
            }
            StorageMode::Hybrid => {
                self.rows
                    .entry(container_id)
                    .or_default()
                    .insert(key.clone(), value.clone());
                // HybridPolicy stub: rebuild hot set from WAL records.
                self.hybrid_hot
                    .entry(container_id)
                    .or_default()
                    .insert(key, value);
            }
        }
    }

    pub fn delete(&mut self, container_id: Uuid, key: &[u8]) {
        if self.is_unavailable(container_id) {
            return;
        }
        match self.mode(container_id) {
            StorageMode::Memory => {}
            StorageMode::Persistent => {
                if let Some(m) = self.rows.get_mut(&container_id) {
                    m.remove(key);
                }
            }
            StorageMode::Hybrid => {
                if let Some(m) = self.rows.get_mut(&container_id) {
                    m.remove(key);
                }
                if let Some(m) = self.hybrid_hot.get_mut(&container_id) {
                    m.remove(key);
                }
            }
        }
    }

    pub fn get(&self, container_id: Uuid, key: &[u8]) -> Option<&[u8]> {
        if self.is_unavailable(container_id) {
            return None;
        }
        self.rows
            .get(&container_id)
            .and_then(|m| m.get(key).map(|v| v.as_slice()))
    }

    pub fn len(&self, container_id: Uuid) -> usize {
        self.rows.get(&container_id).map(|m| m.len()).unwrap_or(0)
    }

    pub fn hybrid_hot_len(&self, container_id: Uuid) -> usize {
        self.hybrid_hot
            .get(&container_id)
            .map(|m| m.len())
            .unwrap_or(0)
    }

    pub fn clear_memory_mode(&mut self) {
        let memory_ids: Vec<Uuid> = self
            .modes
            .iter()
            .filter(|(_, m)| matches!(m, StorageMode::Memory))
            .map(|(id, _)| *id)
            .collect();
        for id in memory_ids {
            self.rows.insert(id, HashMap::new());
            self.hybrid_hot.remove(&id);
        }
    }

    /// True when the container has any durable (non-memory) rows.
    pub fn has_content(&self, container_id: Uuid) -> bool {
        if matches!(self.mode(container_id), StorageMode::Memory) {
            return false;
        }
        self.len(container_id) > 0
    }

    /// Export persistent/hybrid durable rows (memory-mode → empty).
    pub fn export_durable(&self, container_id: Uuid) -> HashMap<Vec<u8>, Vec<u8>> {
        if matches!(self.mode(container_id), StorageMode::Memory) {
            return HashMap::new();
        }
        self.rows
            .get(&container_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Replace durable rows for a container (used by restore-fill).
    pub fn replace_durable(
        &mut self,
        container_id: Uuid,
        mode: StorageMode,
        rows: HashMap<Vec<u8>, Vec<u8>>,
    ) {
        self.register_mode(container_id, mode);
        if matches!(mode, StorageMode::Memory) {
            self.rows.insert(container_id, HashMap::new());
            self.hybrid_hot.remove(&container_id);
            return;
        }
        self.rows.insert(container_id, rows.clone());
        if matches!(mode, StorageMode::Hybrid) {
            self.hybrid_hot.insert(container_id, rows);
        } else {
            self.hybrid_hot.remove(&container_id);
        }
        self.unavailable.remove(&container_id);
    }

    /// Drop all durable content for a container (confirm_drop path).
    pub fn clear_content(&mut self, container_id: Uuid) {
        if let Some(m) = self.rows.get_mut(&container_id) {
            m.clear();
        }
        if let Some(m) = self.hybrid_hot.get_mut(&container_id) {
            m.clear();
        }
    }

    pub fn registered_ids(&self) -> Vec<Uuid> {
        self.modes.keys().copied().collect()
    }
}

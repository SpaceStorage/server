//! Dual-write window + cutover gates (010 contracts/dual-write.md).

use crate::error::MigrateError;
use crate::job::JobId;
use crate::quota::QuotaView;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DualWriteWindow {
    pub container_id: Uuid,
    pub job_id: JobId,
    pub install_seq: u64,
    pub target_id: Uuid,
}

#[derive(Clone, Default)]
pub struct DualWriteRegistry {
    by_source: Arc<RwLock<HashMap<Uuid, DualWriteWindow>>>,
}

impl DualWriteRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, window: DualWriteWindow) -> Result<(), MigrateError> {
        let mut map = self.by_source.write();
        if map.contains_key(&window.container_id) {
            return Err(MigrateError::JobInProgress);
        }
        map.insert(window.container_id, window);
        Ok(())
    }

    pub fn unregister(&self, container_id: Uuid) {
        self.by_source.write().remove(&container_id);
    }

    pub fn get(&self, container_id: Uuid) -> Option<DualWriteWindow> {
        self.by_source.read().get(&container_id).cloned()
    }
}

#[derive(Debug, Clone, Default)]
pub struct CutoverGates {
    pub last_applied_source_seq: u64,
    pub source_head_seq: u64,
    pub quota: QuotaView,
    pub additional_bytes: u64,
    pub placement_ok: bool,
    pub target_complete: bool,
    pub dest_name_free_or_swap: bool,
}

impl CutoverGates {
    pub fn check(&self) -> Result<(), MigrateError> {
        if self.last_applied_source_seq < self.source_head_seq {
            return Err(MigrateError::InvalidState);
        }
        self.quota.check_fits(self.additional_bytes)?;
        if !self.placement_ok {
            return Err(MigrateError::ConstraintUnsatisfiable);
        }
        if !self.target_complete {
            return Err(MigrateError::InvalidState);
        }
        if !self.dest_name_free_or_swap {
            return Err(MigrateError::NameExists {
                container: "dest".into(),
            });
        }
        Ok(())
    }
}

/// Apply a dual-write; same key+seq is idempotent no-op.
pub fn apply_mapped(
    applied: &mut HashMap<(Vec<u8>, u64), ()>,
    key: Vec<u8>,
    seq: u64,
    value: Vec<u8>,
    dest: &mut HashMap<Vec<u8>, Vec<u8>>,
) -> bool {
    let k = (key.clone(), seq);
    if applied.contains_key(&k) {
        return false;
    }
    dest.insert(key, value);
    applied.insert(k, ());
    true
}

pub fn incomplete_name(job_id: JobId) -> String {
    format!("ss:job:{job_id}")
}

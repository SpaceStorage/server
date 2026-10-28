//! Dual-write window + cutover gates (010 contracts/dual-write.md).

use crate::error::MigrateError;
use crate::job::JobId;
use crate::mapping::{apply_row_transform, MappingSpec};
use crate::quota::QuotaView;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use uuid::Uuid;

/// Optional rewrite applied on interceptor puts before target apply / buffer (T076).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DualWriteRewrite {
    pub kind: String,
    pub mapping: MappingSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DualWriteWindow {
    pub container_id: Uuid,
    pub job_id: JobId,
    pub install_seq: u64,
    pub target_id: Uuid,
    /// When set, interceptor puts are rewritten before the target apply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rewrite: Option<DualWriteRewrite>,
}

#[derive(Clone, Default)]
pub struct DualWriteRegistry {
    by_source: Arc<RwLock<HashMap<Uuid, DualWriteWindow>>>,
    /// Offline strategy: refuse new source writes until copy completes.
    paused_writes: Arc<RwLock<HashSet<Uuid>>>,
    /// Monotonic seq assigned to mirrored live writes (after install_seq).
    live_seq: Arc<RwLock<HashMap<Uuid, u64>>>,
    /// Buffered mapped target applies (avoids re-entering the catalog lock).
    pending_target: Arc<RwLock<HashMap<Uuid, HashMap<Vec<u8>, Vec<u8>>>>>,
    /// Bytes added to the source by acknowledged dual-write puts (grown size, T077).
    live_added_bytes: Arc<RwLock<HashMap<Uuid, u64>>>,
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
        self.live_seq
            .write()
            .insert(window.container_id, window.install_seq);
        self.live_added_bytes
            .write()
            .insert(window.container_id, 0);
        map.insert(window.container_id, window);
        Ok(())
    }

    /// Remove the window and return any undrained mapped pending target puts (T076).
    pub fn unregister(&self, container_id: Uuid) -> HashMap<Vec<u8>, Vec<u8>> {
        let target = self
            .by_source
            .write()
            .remove(&container_id)
            .map(|w| w.target_id);
        self.paused_writes.write().remove(&container_id);
        self.live_seq.write().remove(&container_id);
        self.live_added_bytes.write().remove(&container_id);
        if let Some(t) = target {
            self.pending_target
                .write()
                .remove(&t)
                .unwrap_or_default()
        } else {
            HashMap::new()
        }
    }

    pub fn get(&self, container_id: Uuid) -> Option<DualWriteWindow> {
        self.by_source.read().get(&container_id).cloned()
    }

    pub fn pause_writes(&self, container_id: Uuid) {
        self.paused_writes.write().insert(container_id);
    }

    pub fn resume_writes(&self, container_id: Uuid) {
        self.paused_writes.write().remove(&container_id);
    }

    pub fn writes_paused(&self, container_id: Uuid) -> bool {
        self.paused_writes.read().contains(&container_id)
    }

    /// Buffer a mapped apply for `target_id` (used by the coordinating write path).
    pub fn buffer_target_put(&self, target_id: Uuid, key: Vec<u8>, value: Vec<u8>) {
        self.pending_target
            .write()
            .entry(target_id)
            .or_default()
            .insert(key, value);
    }

    pub fn pending_for(&self, target_id: Uuid) -> HashMap<Vec<u8>, Vec<u8>> {
        self.pending_target
            .read()
            .get(&target_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Take and clear pending target puts (drain step; further puts may refill).
    pub fn take_pending(&self, target_id: Uuid) -> HashMap<Vec<u8>, Vec<u8>> {
        self.pending_target
            .write()
            .remove(&target_id)
            .unwrap_or_default()
    }

    /// Baseline source size plus bytes from acknowledged dual-write puts (T077).
    pub fn grown_source_bytes(&self, source_id: Uuid, baseline: u64) -> u64 {
        let added = self
            .live_added_bytes
            .read()
            .get(&source_id)
            .copied()
            .unwrap_or(0);
        baseline.saturating_add(added)
    }

    /// Coordinating write path: refuse offline-paused sources; when a window is
    /// registered, apply the mapped write on the target (caller supplies quorum /
    /// durable apply) before the source ack. Target failure → `Err` (caller must
    /// not ack) and invokes `on_target_fail` (pause job with named error).
    pub fn intercept_source_write<A, F>(
        &self,
        source_id: Uuid,
        key: &[u8],
        value: &[u8],
        apply_target: A,
        on_target_fail: F,
    ) -> Result<Option<u64>, MigrateError>
    where
        A: FnOnce(Uuid, &[u8], &[u8]) -> Result<(), MigrateError>,
        F: FnOnce(JobId, MigrateError),
    {
        if self.writes_paused(source_id) {
            return Err(MigrateError::NotSupported {
                what: "source writes while offline migration runs".into(),
            });
        }
        let Some(window) = self.get(source_id) else {
            return Ok(None);
        };
        let seq = {
            let mut map = self.live_seq.write();
            let s = map.entry(source_id).or_insert(window.install_seq);
            *s = s.saturating_add(1);
            *s
        };
        // Apply rewrite on interceptor puts (T076); migrate windows leave rewrite unset.
        let (mapped_key, mapped_val) = if let Some(rw) = &window.rewrite {
            apply_row_transform(&rw.kind, &rw.mapping, key, value)
        } else {
            (key.to_vec(), value.to_vec())
        };
        // Target apply at that container's write quorum / durable filter first.
        if let Err(e) = apply_target(window.target_id, &mapped_key, &mapped_val) {
            on_target_fail(window.job_id, e.clone());
            return Err(e);
        }
        // Buffer mapped bytes for runner / cutover drain after a successful target apply.
        self.buffer_target_put(window.target_id, mapped_key, mapped_val);
        {
            let mut added = self.live_added_bytes.write();
            let entry = added.entry(source_id).or_insert(0);
            *entry = entry.saturating_add((key.len() + value.len()) as u64);
        }
        Ok(Some(seq))
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

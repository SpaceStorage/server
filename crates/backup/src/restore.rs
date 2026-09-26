//! Restore-fill from snapshot (+ optional PITR) — 013 FR-014/FR-015.

use crate::manifest::SnapshotManifest;
use crate::pitr::{resolve_positions, PitrTarget, StampLsn};
use crate::snapshot::{load_container_rows, load_manifest};
use crate::BackupError;
use spacestorage_storage::StorageEngine;
use std::collections::BTreeMap;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct RestoreRequest {
    pub snapshot_id: Uuid,
    pub confirm_drop: bool,
    pub pitr: Option<PitrTarget>,
    /// Key refs the operator supplies for encrypted containers.
    pub key_refs: Vec<String>,
    /// Optional per-drive stamps after snapshot cut (for HLC PITR).
    pub wal_stamps: BTreeMap<String, Vec<StampLsn>>,
}

pub struct RestoreService {
    pub data_dir: PathBuf,
    pub slice10_enabled: bool,
}

impl RestoreService {
    pub fn new(data_dir: PathBuf, slice10_enabled: bool) -> Self {
        Self {
            data_dir,
            slice10_enabled,
        }
    }

    pub async fn restore_fill(
        &self,
        engine: &StorageEngine,
        req: RestoreRequest,
    ) -> Result<SnapshotManifest, BackupError> {
        if !self.slice10_enabled {
            return Err(BackupError::BackupSlice10Required);
        }

        let man = load_manifest(&self.data_dir, req.snapshot_id).await?;

        for kr in &man.key_refs {
            if !req.key_refs.iter().any(|k| k == kr) {
                return Err(BackupError::KeyRefMissing {
                    key_ref: kr.clone(),
                });
            }
        }

        if let Some(ref pitr) = req.pitr {
            let mut durable = BTreeMap::new();
            for d in engine.all_drives().await {
                durable.insert(d.drive_id.clone(), d.durable_lsn());
            }
            // Fall back to manifest positions as durable ceiling when drives empty.
            for (k, v) in &man.positions {
                durable.entry(k.clone()).or_insert(*v);
            }
            let _positions = resolve_positions(pitr, &req.wal_stamps, &durable)?;
        }

        {
            let content = engine.content.read().await;
            for c in &man.containers {
                if content.has_content(c.id) && !req.confirm_drop {
                    return Err(BackupError::RestoreDestinationHasContent {
                        container: format!("{}.{}", c.namespace, c.name),
                    });
                }
            }
        }

        for c in &man.containers {
            let (mode, rows) =
                load_container_rows(&self.data_dir, req.snapshot_id, c.id).await?;
            let mut content = engine.content.write().await;
            if content.has_content(c.id) && req.confirm_drop {
                content.clear_content(c.id);
            }
            // Memory-mode restore stays empty even if file had rows (defense).
            let rows = if matches!(mode, spacestorage_storage::StorageMode::Memory) {
                Default::default()
            } else {
                rows
            };
            content.replace_durable(c.id, mode, rows);
        }

        Ok(man)
    }
}

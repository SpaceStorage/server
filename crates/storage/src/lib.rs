//! Local storage: per-drive WAL, restore, disk state (013).
//!
//! Supersedes provisional `crates/wal` single-file `wal.log`: path is
//! `{drive.path}/wal/<seq>.wal` (implicit drive `default` = `storage.data_dir`).

pub mod checkpoint;
pub mod compaction;
pub mod content;
pub mod definitions;
pub mod disk;
pub mod error;
pub mod kv_put;
pub mod legal_hold;
pub mod metrics;
pub mod mode;
pub mod quarantine;
pub mod restore;
pub mod tombstone;
pub mod ttl;
pub mod wal;

pub use content::{ContentStore, UnavailableReason};
pub use definitions::{append_definition, load_definitions, PersistedDefinition};
pub use disk::{DiskState, DriveDiskState};
pub use error::StorageError;
pub use legal_hold::{EraseRecord, EraseRequest, LegalError, LegalHold, LegalHoldStore};
pub use metrics::WalMetrics;
pub use mode::StorageMode;
pub use spacestorage_types::AckKind;
pub use wal::{DriveWal, DurableAck, SyncMode, WalRecord, WalRecordType};

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// Opened drives keyed by drive_id; retains restored content across ready.
pub struct StorageEngine {
    pub drives: RwLock<HashMap<String, Arc<DriveWal>>>,
    pub disk: RwLock<HashMap<String, DriveDiskState>>,
    pub content: RwLock<ContentStore>,
    pub metrics: Arc<WalMetrics>,
    pub node_degraded: RwLock<bool>,
}

impl StorageEngine {
    pub fn new(metrics: Arc<WalMetrics>) -> Self {
        Self {
            drives: RwLock::new(HashMap::new()),
            disk: RwLock::new(HashMap::new()),
            content: RwLock::new(ContentStore::default()),
            metrics,
            node_degraded: RwLock::new(false),
        }
    }

    /// Open implicit `default` drive at `data_dir`, and/or explicit drive list.
    pub async fn open_drives(
        &self,
        data_dir: Option<&Path>,
        drives: &[(String, PathBuf)],
        sync: SyncMode,
        group_max_wait: Duration,
        group_max_bytes: usize,
        node_id: &str,
    ) -> Result<(), StorageError> {
        let mut list: Vec<(String, PathBuf)> = drives.to_vec();
        if list.is_empty() {
            if let Some(dir) = data_dir {
                list.push(("default".into(), dir.to_path_buf()));
            } else {
                return Ok(());
            }
        }

        let mut map = self.drives.write().await;
        let mut disk = self.disk.write().await;
        for (id, path) in list {
            if map.contains_key(&id) {
                continue;
            }
            let wal = match DriveWal::open(
                id.clone(),
                path,
                sync,
                group_max_wait,
                group_max_bytes,
                node_id,
                Arc::clone(&self.metrics),
            )
            .await
            {
                Ok(w) => w,
                Err(StorageError::WalCorrupt(_)) => {
                    disk.insert(
                        id.clone(),
                        DriveDiskState {
                            drive_id: id.clone(),
                            state: DiskState::Corrupt,
                            quarantined: Vec::new(),
                        },
                    );
                    *self.node_degraded.write().await = true;
                    continue;
                }
                Err(e) => return Err(e),
            };
            let quarantined = wal.take_quarantined();
            let state = if quarantined.is_empty() {
                DiskState::Ok
            } else {
                *self.node_degraded.write().await = true;
                DiskState::Corrupt
            };
            disk.insert(
                id.clone(),
                DriveDiskState {
                    drive_id: id.clone(),
                    state,
                    quarantined,
                },
            );
            map.insert(id, Arc::new(wal));
        }
        Ok(())
    }

    pub async fn get(&self, drive_id: &str) -> Option<Arc<DriveWal>> {
        self.drives.read().await.get(drive_id).cloned()
    }

    pub async fn all_drives(&self) -> Vec<Arc<DriveWal>> {
        self.drives.read().await.values().cloned().collect()
    }

    pub async fn mark_full(&self, drive_id: &str) {
        if let Some(d) = self.disk.write().await.get_mut(drive_id) {
            d.state = DiskState::Full;
        }
        *self.node_degraded.write().await = true;
        self.metrics
            .wal_disk_full_total
            .fetch_add(1, Ordering::Relaxed);
    }

    pub async fn mark_corrupt(&self, drive_id: &str, note: &str) {
        if let Some(d) = self.disk.write().await.get_mut(drive_id) {
            d.state = DiskState::Corrupt;
            d.quarantined.push(note.to_string());
        }
        *self.node_degraded.write().await = true;
    }

    /// Append a KV put to the first available drive WAL and update content (client durability).
    pub async fn put_kv_durable(
        &self,
        container_id: uuid::Uuid,
        key: &str,
        value: &[u8],
    ) -> Result<(), StorageError> {
        let drives = self.all_drives().await;
        let wal = drives
            .first()
            .ok_or_else(|| StorageError::WalCorrupt("no drive open".into()))?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        crate::kv_put::put_durable(
            wal,
            container_id,
            stamp,
            key.as_bytes(),
            value,
            spacestorage_types::StorageModeChoice::Persistent,
            None,
        )
        .await?;
        let mut content = self.content.write().await;
        content.register_mode(container_id, StorageMode::Persistent);
        content.put(container_id, key.as_bytes().to_vec(), value.to_vec());
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Checkpoint {
    pub drive_id: String,
    pub covered_lsn: u64,
    pub at_hlc: u64,
}

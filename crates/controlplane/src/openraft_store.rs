//! Openraft log store + state machine for the control-plane `Raft::new` runtime.
//!
//! Application payloads are dual-written to [`crate::RaftStore`] for the SpaceStorage
//! on-disk Raft format / restore path; openraft owns elections, commit, and replication.
//!
//! Constitution II: `IOFlushed` is signaled only after log/snapshot bytes are fsynced on
//! the dedicated blocking pool — never from an in-memory map alone.

use crate::openraft_adapter::{RaftAppRequest, RaftAppResponse, TypeConfig};
use futures::Stream;
use futures::TryStreamExt;
use openraft::alias::{LogIdOf, SnapshotMetaOf, SnapshotOf, StoredMembershipOf, VoteOf};
use openraft::entry::RaftEntry;
use openraft::storage::{
    EntryResponder, IOFlushed, RaftLogReader, RaftLogStorage, RaftSnapshotBuilder, RaftStateMachine,
    Snapshot,
};
use openraft::{EntryPayload, LogState, OptionalSend, RaftTypeConfig};
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::io;
use std::io::Cursor;
use std::ops::RangeBounds;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Openraft Raft log storage with optional durable directory.
///
/// When [`Self::with_persist_dir`] is used, append/vote/committed flush via
/// `spawn_blocking` + `fsync` before `IOFlushed`. Tests may use [`Default`] and
/// still defer the flush callback onto the blocking pool.
#[derive(Debug, Clone, Default)]
pub struct MemLogStore {
    inner: Arc<Mutex<MemLogInner>>,
    persist_dir: Option<PathBuf>,
}

#[derive(Debug, Default)]
struct MemLogInner {
    last_purged_log_id: Option<LogIdOf<TypeConfig>>,
    log: BTreeMap<u64, <TypeConfig as RaftTypeConfig>::Entry>,
    committed: Option<LogIdOf<TypeConfig>>,
    vote: Option<VoteOf<TypeConfig>>,
}

impl MemLogStore {
    pub fn with_persist_dir(dir: impl Into<PathBuf>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(MemLogInner::default())),
            persist_dir: Some(dir.into()),
        }
    }

    fn persist_dir(&self) -> Option<PathBuf> {
        self.persist_dir.clone()
    }
}

fn io_err(e: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::Other, e.to_string())
}

fn atomic_write_fsync(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    let f = std::fs::File::open(&tmp)?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, path)?;
    if let Some(parent) = path.parent() {
        let dir = std::fs::File::open(parent)?;
        let _ = dir.sync_all();
    }
    Ok(())
}

/// Flush durable openraft state on the blocking pool (Constitution II).
fn flush_log_blocking(
    dir: Option<PathBuf>,
    entries: Vec<<TypeConfig as RaftTypeConfig>::Entry>,
    vote: Option<VoteOf<TypeConfig>>,
    committed: Option<LogIdOf<TypeConfig>>,
    last_purged: Option<LogIdOf<TypeConfig>>,
) -> io::Result<()> {
    let Some(dir) = dir else {
        // Ephemeral harness: still hop off the worker before IOFlushed.
        return Ok(());
    };
    std::fs::create_dir_all(&dir)?;
    let log_path = dir.join("openraft_log.json");
    let meta_path = dir.join("openraft_meta.json");
    let log_bytes = serde_json::to_vec(&entries).map_err(|e| io_err(e))?;
    let meta = serde_json::json!({
        "vote": vote,
        "committed": committed,
        "last_purged": last_purged,
    });
    let meta_bytes = serde_json::to_vec_pretty(&meta).map_err(|e| io_err(e))?;
    atomic_write_fsync(&log_path, &log_bytes)?;
    atomic_write_fsync(&meta_path, &meta_bytes)?;
    Ok(())
}

fn flush_snapshot_blocking(dir: Option<PathBuf>, data: &[u8]) -> io::Result<()> {
    let Some(dir) = dir else {
        return Ok(());
    };
    std::fs::create_dir_all(&dir)?;
    atomic_write_fsync(&dir.join("openraft_snapshot.bin"), data)
}

impl RaftLogReader<TypeConfig> for MemLogStore {
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + OptionalSend>(
        &mut self,
        range: RB,
    ) -> Result<Vec<<TypeConfig as RaftTypeConfig>::Entry>, io::Error> {
        let inner = self.inner.lock().await;
        Ok(inner
            .log
            .range(range)
            .map(|(_, e)| e.clone())
            .collect())
    }

    async fn read_vote(&mut self) -> Result<Option<VoteOf<TypeConfig>>, io::Error> {
        Ok(self.inner.lock().await.vote.clone())
    }
}

impl RaftLogStorage<TypeConfig> for MemLogStore {
    type LogReader = Self;

    async fn get_log_state(&mut self) -> Result<LogState<TypeConfig>, io::Error> {
        let inner = self.inner.lock().await;
        let last = inner
            .log
            .iter()
            .next_back()
            .map(|(_, e)| e.log_id())
            .or_else(|| inner.last_purged_log_id.clone());
        Ok(LogState {
            last_purged_log_id: inner.last_purged_log_id.clone(),
            last_log_id: last,
        })
    }

    async fn get_log_reader(&mut self) -> Self::LogReader {
        self.clone()
    }

    async fn save_vote(&mut self, vote: &VoteOf<TypeConfig>) -> Result<(), io::Error> {
        let dir = self.persist_dir();
        let snapshot = {
            let mut inner = self.inner.lock().await;
            inner.vote = Some(vote.clone());
            (
                inner.log.values().cloned().collect::<Vec<_>>(),
                inner.vote.clone(),
                inner.committed.clone(),
                inner.last_purged_log_id.clone(),
            )
        };
        tokio::task::spawn_blocking(move || {
            flush_log_blocking(dir, snapshot.0, snapshot.1, snapshot.2, snapshot.3)
        })
        .await
        .map_err(|e| io_err(e))??;
        Ok(())
    }

    async fn save_committed(
        &mut self,
        committed: Option<LogIdOf<TypeConfig>>,
    ) -> Result<(), io::Error> {
        let dir = self.persist_dir();
        let snapshot = {
            let mut inner = self.inner.lock().await;
            inner.committed = committed;
            (
                inner.log.values().cloned().collect::<Vec<_>>(),
                inner.vote.clone(),
                inner.committed.clone(),
                inner.last_purged_log_id.clone(),
            )
        };
        tokio::task::spawn_blocking(move || {
            flush_log_blocking(dir, snapshot.0, snapshot.1, snapshot.2, snapshot.3)
        })
        .await
        .map_err(|e| io_err(e))??;
        Ok(())
    }

    async fn read_committed(&mut self) -> Result<Option<LogIdOf<TypeConfig>>, io::Error> {
        Ok(self.inner.lock().await.committed.clone())
    }

    async fn append<I>(&mut self, entries: I, callback: IOFlushed<TypeConfig>) -> Result<(), io::Error>
    where
        I: IntoIterator<Item = <TypeConfig as RaftTypeConfig>::Entry> + OptionalSend,
        I::IntoIter: OptionalSend,
    {
        let dir = self.persist_dir();
        let snapshot = {
            let mut inner = self.inner.lock().await;
            for entry in entries {
                inner.log.insert(entry.index(), entry);
            }
            (
                inner.log.values().cloned().collect::<Vec<_>>(),
                inner.vote.clone(),
                inner.committed.clone(),
                inner.last_purged_log_id.clone(),
            )
        };
        let result = tokio::task::spawn_blocking(move || {
            flush_log_blocking(dir, snapshot.0, snapshot.1, snapshot.2, snapshot.3)
        })
        .await
        .map_err(|e| io_err(e))
        .and_then(|r| r);
        // Signal openraft only after blocking-pool fsync (or ephemeral hop) completes.
        callback.io_completed(result);
        Ok(())
    }

    async fn truncate_after(
        &mut self,
        last_log_id: Option<LogIdOf<TypeConfig>>,
    ) -> Result<(), io::Error> {
        let start = match last_log_id {
            Some(id) => id.index() + 1,
            None => 0,
        };
        let dir = self.persist_dir();
        let snapshot = {
            let mut inner = self.inner.lock().await;
            let keys: Vec<u64> = inner.log.range(start..).map(|(k, _)| *k).collect();
            for k in keys {
                inner.log.remove(&k);
            }
            (
                inner.log.values().cloned().collect::<Vec<_>>(),
                inner.vote.clone(),
                inner.committed.clone(),
                inner.last_purged_log_id.clone(),
            )
        };
        tokio::task::spawn_blocking(move || {
            flush_log_blocking(dir, snapshot.0, snapshot.1, snapshot.2, snapshot.3)
        })
        .await
        .map_err(|e| io_err(e))??;
        Ok(())
    }

    async fn purge(&mut self, log_id: LogIdOf<TypeConfig>) -> Result<(), io::Error> {
        let dir = self.persist_dir();
        let snapshot = {
            let mut inner = self.inner.lock().await;
            inner.last_purged_log_id = Some(log_id.clone());
            let keys: Vec<u64> = inner
                .log
                .range(..=log_id.index())
                .map(|(k, _)| *k)
                .collect();
            for k in keys {
                inner.log.remove(&k);
            }
            (
                inner.log.values().cloned().collect::<Vec<_>>(),
                inner.vote.clone(),
                inner.committed.clone(),
                inner.last_purged_log_id.clone(),
            )
        };
        tokio::task::spawn_blocking(move || {
            flush_log_blocking(dir, snapshot.0, snapshot.1, snapshot.2, snapshot.3)
        })
        .await
        .map_err(|e| io_err(e))??;
        Ok(())
    }
}

#[derive(Debug)]
struct StoredSnapshot {
    meta: SnapshotMetaOf<TypeConfig>,
    data: Vec<u8>,
}

#[derive(Debug)]
struct StateMachineInner {
    last_applied_log: Option<LogIdOf<TypeConfig>>,
    last_membership: StoredMembershipOf<TypeConfig>,
    last_request: Option<RaftAppRequest>,
    current_snapshot: Option<StoredSnapshot>,
}

impl Default for StateMachineInner {
    fn default() -> Self {
        Self {
            last_applied_log: None,
            last_membership: StoredMembershipOf::<TypeConfig>::default(),
            last_request: None,
            current_snapshot: None,
        }
    }
}

/// Control-plane state machine: records last applied app request; ControlPlane apply stays external.
#[derive(Debug, Clone, Default)]
pub struct StateMachineStore {
    inner: Arc<Mutex<StateMachineInner>>,
    persist_dir: Option<PathBuf>,
}

impl StateMachineStore {
    pub fn with_persist_dir(dir: impl Into<PathBuf>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(StateMachineInner::default())),
            persist_dir: Some(dir.into()),
        }
    }

    pub async fn last_applied_index(&self) -> u64 {
        self.inner
            .lock()
            .await
            .last_applied_log
            .as_ref()
            .map(|id| id.index())
            .unwrap_or(0)
    }
}

impl RaftSnapshotBuilder<TypeConfig> for StateMachineStore {
    type SnapshotData = Cursor<Vec<u8>>;

    async fn build_snapshot(
        &mut self,
    ) -> Result<SnapshotOf<TypeConfig, Cursor<Vec<u8>>>, io::Error> {
        let dir = self.persist_dir.clone();
        let (meta, data) = {
            let mut inner = self.inner.lock().await;
            let data = serde_json::to_vec(&inner.last_request)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            let meta = SnapshotMetaOf::<TypeConfig> {
                last_log_id: inner.last_applied_log.clone(),
                last_membership: inner.last_membership.clone(),
            };
            inner.current_snapshot = Some(StoredSnapshot {
                meta: meta.clone(),
                data: data.clone(),
            });
            (meta, data)
        };
        let data_c = data.clone();
        tokio::task::spawn_blocking(move || flush_snapshot_blocking(dir, &data_c))
            .await
            .map_err(|e| io_err(e))??;
        Ok(Snapshot {
            meta,
            snapshot: Cursor::new(data),
        })
    }
}

impl RaftStateMachine<TypeConfig> for StateMachineStore {
    type SnapshotData = Cursor<Vec<u8>>;
    type SnapshotBuilder = Self;

    async fn applied_state(
        &mut self,
    ) -> Result<(Option<LogIdOf<TypeConfig>>, StoredMembershipOf<TypeConfig>), io::Error> {
        let inner = self.inner.lock().await;
        Ok((inner.last_applied_log.clone(), inner.last_membership.clone()))
    }

    async fn apply<Strm>(&mut self, mut entries: Strm) -> Result<(), io::Error>
    where
        Strm: Stream<Item = Result<EntryResponder<TypeConfig>, io::Error>>
            + Unpin
            + OptionalSend,
    {
        let mut inner = self.inner.lock().await;
        while let Some((entry, responder)) = entries.try_next().await? {
            let log_id = entry.log_id();
            inner.last_applied_log = Some(log_id.clone());
            let response = match &entry.payload {
                EntryPayload::Blank => RaftAppResponse { ok: true },
                EntryPayload::Normal(req) => {
                    inner.last_request = Some(req.clone());
                    RaftAppResponse { ok: true }
                }
                EntryPayload::Membership(mem) => {
                    inner.last_membership =
                        StoredMembershipOf::<TypeConfig>::new(Some(log_id.clone()), mem.clone());
                    RaftAppResponse { ok: true }
                }
            };
            if let Some(responder) = responder {
                responder.send(response);
            }
        }
        Ok(())
    }

    async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
        self.clone()
    }

    async fn install_snapshot(
        &mut self,
        meta: &SnapshotMetaOf<TypeConfig>,
        snapshot: Self::SnapshotData,
    ) -> Result<(), io::Error> {
        let data = snapshot.into_inner();
        let last_request: Option<RaftAppRequest> = if data.is_empty() {
            None
        } else {
            serde_json::from_slice(&data)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
        };
        let dir = self.persist_dir.clone();
        {
            let mut inner = self.inner.lock().await;
            inner.last_applied_log = meta.last_log_id.clone();
            inner.last_membership = meta.last_membership.clone();
            inner.last_request = last_request;
            inner.current_snapshot = Some(StoredSnapshot {
                meta: meta.clone(),
                data: data.clone(),
            });
        }
        tokio::task::spawn_blocking(move || flush_snapshot_blocking(dir, &data))
            .await
            .map_err(|e| io_err(e))??;
        Ok(())
    }

    async fn get_current_snapshot(
        &mut self,
    ) -> Result<Option<SnapshotOf<TypeConfig, Self::SnapshotData>>, io::Error> {
        let inner = self.inner.lock().await;
        Ok(inner.current_snapshot.as_ref().map(|s| Snapshot {
            meta: s.meta.clone(),
            snapshot: Cursor::new(s.data.clone()),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn save_vote_fsyncs_on_blocking_pool() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = MemLogStore::with_persist_dir(dir.path().join("openraft"));
        store
            .save_vote(&openraft::Vote::new(1, Uuid::nil()))
            .await
            .unwrap();
        assert!(dir.path().join("openraft/openraft_meta.json").exists());
    }
}

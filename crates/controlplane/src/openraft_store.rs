//! In-memory openraft log store + state machine for the control-plane `Raft::new` runtime.
//!
//! SpaceStorage still dual-writes application payloads to [`crate::RaftStore`] for the
//! on-disk Raft format / restore path; openraft owns elections, commit, and replication.

use crate::openraft_adapter::{RaftAppRequest, RaftAppResponse, TypeConfig};
use futures::Stream;
use futures::TryStreamExt;
use openraft::alias::{LogIdOf, SnapshotMetaOf, SnapshotOf, StoredMembershipOf, VoteOf};
use openraft::entry::RaftEntry;
use openraft::storage::{EntryResponder, IOFlushed, RaftLogReader, RaftLogStorage, RaftSnapshotBuilder, RaftStateMachine, Snapshot};
use openraft::{EntryPayload, LogState, OptionalSend, RaftTypeConfig};
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::io;
use std::io::Cursor;
use std::ops::RangeBounds;
use std::sync::Arc;
use tokio::sync::Mutex;

/// In-memory Raft log storage (openraft 0.10).
#[derive(Debug, Clone, Default)]
pub struct MemLogStore {
    inner: Arc<Mutex<MemLogInner>>,
}

#[derive(Debug, Default)]
struct MemLogInner {
    last_purged_log_id: Option<LogIdOf<TypeConfig>>,
    log: BTreeMap<u64, <TypeConfig as RaftTypeConfig>::Entry>,
    committed: Option<LogIdOf<TypeConfig>>,
    vote: Option<VoteOf<TypeConfig>>,
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
        self.inner.lock().await.vote = Some(vote.clone());
        Ok(())
    }

    async fn save_committed(
        &mut self,
        committed: Option<LogIdOf<TypeConfig>>,
    ) -> Result<(), io::Error> {
        self.inner.lock().await.committed = committed;
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
        let mut inner = self.inner.lock().await;
        for entry in entries {
            inner.log.insert(entry.index(), entry);
        }
        callback.io_completed(Ok(()));
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
        let mut inner = self.inner.lock().await;
        let keys: Vec<u64> = inner.log.range(start..).map(|(k, _)| *k).collect();
        for k in keys {
            inner.log.remove(&k);
        }
        Ok(())
    }

    async fn purge(&mut self, log_id: LogIdOf<TypeConfig>) -> Result<(), io::Error> {
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
}

impl StateMachineStore {
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
        let mut inner = self.inner.lock().await;
        inner.last_applied_log = meta.last_log_id.clone();
        inner.last_membership = meta.last_membership.clone();
        inner.last_request = last_request;
        inner.current_snapshot = Some(StoredSnapshot {
            meta: meta.clone(),
            data,
        });
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

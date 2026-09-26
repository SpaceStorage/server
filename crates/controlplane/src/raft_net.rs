//! Raft RPCs over existing internodes (no new port). See contracts/raft-rpc.md.

use crate::group::GroupId;
use crate::raft_store::RaftLogRecord;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Wire message kinds registered in `spacestorage_internode::registry`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "rpc", rename_all = "snake_case")]
pub enum RaftRpc {
    Vote(RaftVote),
    Append(RaftAppend),
    Snapshot(RaftSnapshot),
    Forward(RaftForward),
    MetricsPush(MetricsPush),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RaftVote {
    pub group: GroupId,
    pub term: u64,
    pub candidate_id: Uuid,
    pub last_log_index: u64,
    pub last_log_term: u64,
    /// Response fields (zero/false when request).
    pub vote_granted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RaftAppend {
    pub group: GroupId,
    pub term: u64,
    pub leader_id: Uuid,
    pub prev_log_index: u64,
    pub prev_log_term: u64,
    pub entries: Vec<RaftLogRecord>,
    pub leader_commit: u64,
    /// Follower reply.
    pub success: bool,
    pub match_index: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RaftSnapshot {
    pub group: GroupId,
    pub term: u64,
    pub leader_id: Uuid,
    pub last_included_index: u64,
    pub last_included_term: u64,
    pub offset: u64,
    pub data: Vec<u8>,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RaftForward {
    pub group: GroupId,
    pub body: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MetricsPush {
    pub namespace_id: Uuid,
    pub series: serde_json::Value,
}

/// In-process network for conformance / single-binary multi-node harness.
#[derive(Default)]
pub struct InProcessRaftNet {
    /// Peer mailboxes: node → pending inbound RPCs.
    mailboxes: parking_lot::Mutex<std::collections::BTreeMap<Uuid, Vec<(Uuid, RaftRpc)>>>,
}

impl InProcessRaftNet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn send(&self, from: Uuid, to: Uuid, rpc: RaftRpc) {
        self.mailboxes.lock().entry(to).or_default().push((from, rpc));
    }

    pub fn drain(&self, node: Uuid) -> Vec<(Uuid, RaftRpc)> {
        self.mailboxes
            .lock()
            .remove(&node)
            .unwrap_or_default()
    }
}

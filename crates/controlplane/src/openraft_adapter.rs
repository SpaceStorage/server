//! Openraft 0.10 adapters + `Raft::new` factory for the control-plane Raft seam (006).
//!
//! Production groups own a real [`openraft::Raft`] runtime (see [`crate::RaftGroup`]).
//! Peer RPCs for multi-node clusters still go through internodes; the factory below
//! returns unreachable for remote targets until a wire network is plugged in — single-node
//! first-binary / bootstrap paths are fully served by openraft.

use crate::group::GroupId;
use crate::openraft_store::{MemLogStore, StateMachineStore};
use crate::raft_net::{InProcessRaftNet, RaftRpc};
use crate::raft_store::{RaftBody, RaftLogRecord};
use crate::error::{ControlPlaneError, Result};
use openraft::alias::{SnapshotOf, VoteOf};
use openraft::error::{RPCError, ReplicationClosed, StreamingError, Unreachable};
use openraft::network::v2::RaftNetworkV2;
use openraft::network::{RaftNetworkFactory, RPCOption};
use openraft::raft::{AppendEntriesRequest, AppendEntriesResponse, SnapshotResponse, VoteRequest, VoteResponse};
use openraft::{BasicNode, Config as OpenRaftConfig, OptionalSend, Raft, ServerState};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// Application log payload mirrored from cluster / namespace ops.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RaftAppRequest {
    pub group: GroupId,
    pub body: RaftBody,
}

impl fmt::Display for RaftAppRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RaftAppRequest({:?})", self.group)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RaftAppResponse {
    pub ok: bool,
}

impl fmt::Display for RaftAppResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RaftAppResponse(ok={})", self.ok)
    }
}

openraft::declare_raft_types!(
    /// SpaceStorage control-plane Raft type config (openraft 0.10).
    pub TypeConfig: D = RaftAppRequest, R = RaftAppResponse, NodeId = Uuid
);

/// Validate the openraft runtime [`OpenRaftConfig`] used by [`Raft::new`].
pub fn openraft_config() -> std::result::Result<OpenRaftConfig, openraft::ConfigError> {
    let mut c = OpenRaftConfig::default();
    c.cluster_name = "spacestorage-controlplane".into();
    c.heartbeat_interval = 50;
    c.election_timeout_min = 150;
    c.election_timeout_max = 300;
    c.validate()
}

/// Active cargo pin (see `crates/controlplane/Cargo.toml`).
pub const OPENRAFT_ACTIVE_PIN: &str = "0.10.0-alpha.35";

/// Handle type owned by [`crate::RaftGroup`].
pub type ControlRaft = Raft<TypeConfig, StateMachineStore>;

/// Build and spawn an openraft runtime for one control-plane group (single-node capable).
pub async fn start_raft(node_id: Uuid) -> Result<ControlRaft> {
    let config = Arc::new(
        openraft_config().map_err(|e| ControlPlaneError::Msg(format!("openraft config: {e}")))?,
    );
    let log_store = MemLogStore::default();
    let state_machine = StateMachineStore::default();
    let network = LocalNetworkFactory;
    Raft::new(node_id, config, network, log_store, state_machine)
        .await
        .map_err(|e| ControlPlaneError::Msg(format!("Raft::new: {e}")))
}

/// Initialize membership `{self}` and wait until this node is Leader.
pub async fn bootstrap_single_voter(raft: &ControlRaft, node_id: Uuid) -> Result<()> {
    let mut members = BTreeMap::new();
    members.insert(node_id, BasicNode::default());
    match raft.initialize(members).await {
        Ok(()) => {}
        Err(e) => {
            // Already initialized on restart is fine.
            let msg = e.to_string();
            if !msg.contains("NotAllowed") && !msg.contains("already") {
                return Err(ControlPlaneError::Msg(format!("raft initialize: {e}")));
            }
        }
    }
    raft.wait(Some(Duration::from_secs(5)))
        .metrics(|m| m.state == ServerState::Leader, "become leader")
        .await
        .map_err(|e| ControlPlaneError::Msg(format!("wait leader: {e}")))?;
    Ok(())
}

/// Network adapter seam used by tests / future internodes wiring (not the openraft factory).
pub struct OpenRaftNetwork {
    pub node_id: Uuid,
    pub net: Arc<InProcessRaftNet>,
}

impl OpenRaftNetwork {
    pub fn new(node_id: Uuid, net: Arc<InProcessRaftNet>) -> Self {
        Self { node_id, net }
    }

    pub fn enqueue(&self, to: Uuid, rpc: RaftRpc) {
        self.net.send(self.node_id, to, rpc);
    }

    pub fn drain_inbound(&self) -> Vec<(Uuid, RaftRpc)> {
        self.net.drain(self.node_id)
    }
}

/// Helper: convert an openraft-facing app request into a durable log record body.
pub fn app_request_to_record(
    group: GroupId,
    term: u64,
    index: u64,
    req: RaftAppRequest,
    hlc: spacestorage_clocks::HlcStamp,
) -> RaftLogRecord {
    RaftLogRecord {
        group,
        term,
        index,
        hlc,
        body: req.body,
    }
}

/// Single-node / stub network: remote RPCs are unreachable (first-binary voter set size 1).
#[derive(Clone, Debug, Default)]
pub struct LocalNetworkFactory;

#[derive(Clone, Debug)]
pub struct LocalNetwork {
    target: Uuid,
}

impl RaftNetworkFactory<TypeConfig> for LocalNetworkFactory {
    type Network = LocalNetwork;

    async fn new_client(&mut self, target: Uuid, _node: &BasicNode) -> Self::Network {
        LocalNetwork { target }
    }
}

impl RaftNetworkV2<TypeConfig> for LocalNetwork {
    type SnapshotData = Cursor<Vec<u8>>;

    async fn append_entries(
        &mut self,
        _rpc: AppendEntriesRequest<TypeConfig>,
        _option: RPCOption,
    ) -> std::result::Result<AppendEntriesResponse<TypeConfig>, RPCError<TypeConfig>> {
        Err(RPCError::Unreachable(Unreachable::from_string(format!(
            "no remote peer {}",
            self.target
        ))))
    }

    async fn vote(
        &mut self,
        _rpc: VoteRequest<TypeConfig>,
        _option: RPCOption,
    ) -> std::result::Result<VoteResponse<TypeConfig>, RPCError<TypeConfig>> {
        Err(RPCError::Unreachable(Unreachable::from_string(format!(
            "no remote peer {}",
            self.target
        ))))
    }

    async fn full_snapshot(
        &mut self,
        _vote: VoteOf<TypeConfig>,
        _snapshot: SnapshotOf<TypeConfig, Self::SnapshotData>,
        _cancel: impl Future<Output = ReplicationClosed> + OptionalSend + 'static,
        _option: RPCOption,
    ) -> std::result::Result<SnapshotResponse<TypeConfig>, StreamingError<TypeConfig>> {
        Err(StreamingError::Unreachable(Unreachable::from_string(
            format!("no remote peer {}", self.target),
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raft_net::RaftVote;

    #[test]
    fn openraft_config_validates() {
        let c = openraft_config().expect("openraft config");
        assert!(c.election_timeout_min > c.heartbeat_interval);
    }

    #[test]
    fn network_adapter_enqueues_vote() {
        let net = Arc::new(InProcessRaftNet::new());
        let a = Uuid::now_v7();
        let b = Uuid::now_v7();
        let adapter = OpenRaftNetwork::new(a, Arc::clone(&net));
        adapter.enqueue(
            b,
            RaftRpc::Vote(RaftVote {
                group: GroupId::Cluster,
                term: 1,
                candidate_id: a,
                last_log_index: 0,
                last_log_term: 0,
                vote_granted: false,
            }),
        );
        let inbox = net.drain(b);
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0].0, a);
    }

    #[test]
    fn pin_is_010() {
        assert!(OPENRAFT_ACTIVE_PIN.starts_with("0.10"));
    }

    #[tokio::test]
    async fn raft_new_bootstraps_single_voter() {
        let id = Uuid::now_v7();
        let raft = start_raft(id).await.expect("Raft::new");
        bootstrap_single_voter(&raft, id)
            .await
            .expect("initialize+leader");
        assert!(raft.is_leader());
    }
}

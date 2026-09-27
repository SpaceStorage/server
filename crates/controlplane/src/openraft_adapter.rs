//! Openraft 0.10 adapters + `Raft::new` factory for the control-plane Raft seam (006).
//!
//! Production groups own a real [`openraft::Raft`] runtime (see [`crate::RaftGroup`]).
//! Multi-node peer RPCs go through [`PeerRaftRegistry`] (in-process) and/or internode
//! frames (`spacestorage_internode::raft_wire`) — AppendEntries / Vote / full Snapshot.

use crate::error::{ControlPlaneError, Result};
use crate::group::GroupId;
use crate::openraft_store::{MemLogStore, StateMachineStore};
use crate::raft_net::{InProcessRaftNet, RaftRpc};
use crate::raft_store::{RaftBody, RaftLogRecord};
use openraft::alias::{SnapshotOf, VoteOf};
use openraft::error::{NetworkError, RPCError, ReplicationClosed, StreamingError, Unreachable};
use openraft::network::v2::RaftNetworkV2;
use openraft::network::{RaftNetworkFactory, RPCOption};
use openraft::raft::{AppendEntriesRequest, AppendEntriesResponse, SnapshotResponse, VoteRequest, VoteResponse};
use openraft::storage::Snapshot;
use openraft::{BasicNode, Config as OpenRaftConfig, OptionalSend, Raft, ServerState};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use spacestorage_internode::{
    decode_raft_body, encode_raft_payload, raft_rpc, tcp_dial_addr, RaftPeerHandler, RaftRpcKind,
};
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

/// Shared map of live Raft handles so peer RPCs can call into remote cores in-process
/// (and so fabric handlers can dispatch inbound internodes frames).
#[derive(Clone, Default)]
pub struct PeerRaftRegistry {
    inner: Arc<RwLock<BTreeMap<Uuid, ControlRaft>>>,
}

impl PeerRaftRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, id: Uuid, raft: ControlRaft) {
        self.inner.write().insert(id, raft);
    }

    pub fn unregister(&self, id: Uuid) {
        self.inner.write().remove(&id);
    }

    pub fn get(&self, id: Uuid) -> Option<ControlRaft> {
        self.inner.read().get(&id).cloned()
    }

    pub fn ids(&self) -> Vec<Uuid> {
        self.inner.read().keys().copied().collect()
    }
}

/// Solo-voter factory (remote peers stay unreachable).
pub async fn start_raft(node_id: Uuid) -> Result<ControlRaft> {
    let config = Arc::new(
        openraft_config().map_err(|e| ControlPlaneError::Msg(format!("openraft config: {e}")))?,
    );
    let log_store = MemLogStore::default();
    let state_machine = StateMachineStore::default();
    Raft::new(node_id, config, LocalNetworkFactory, log_store, state_machine)
        .await
        .map_err(|e| ControlPlaneError::Msg(format!("Raft::new: {e}")))
}

/// Multi-node capable factory backed by [`PeerRaftRegistry`] and/or live TCP dial.
pub async fn start_raft_peered(
    node_id: Uuid,
    registry: Arc<PeerRaftRegistry>,
) -> Result<ControlRaft> {
    start_raft_peered_with_secret(node_id, registry, Arc::new(Vec::new())).await
}

/// Same as [`start_raft_peered`] but with a join secret for TCP dial auth.
pub async fn start_raft_peered_with_secret(
    node_id: Uuid,
    registry: Arc<PeerRaftRegistry>,
    join_secret: Arc<Vec<u8>>,
) -> Result<ControlRaft> {
    let config = Arc::new(
        openraft_config().map_err(|e| ControlPlaneError::Msg(format!("openraft config: {e}")))?,
    );
    let log_store = MemLogStore::default();
    let state_machine = StateMachineStore::default();
    Raft::new(
        node_id,
        config,
        PeerNetworkFactory {
            registry,
            join_secret,
        },
        log_store,
        state_machine,
    )
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

/// Initialize a multi-voter membership and wait until any registered peer is Leader.
pub async fn bootstrap_multi_voter(
    raft: &ControlRaft,
    members: BTreeMap<Uuid, BasicNode>,
) -> Result<()> {
    match raft.initialize(members).await {
        Ok(()) => {}
        Err(e) => {
            let msg = e.to_string();
            if !msg.contains("NotAllowed") && !msg.contains("already") {
                return Err(ControlPlaneError::Msg(format!("raft initialize: {e}")));
            }
        }
    }
    Ok(())
}

/// Wait until this node observes a Leader (self or peer).
pub async fn wait_for_leader(raft: &ControlRaft, timeout: Duration) -> Result<Uuid> {
    let m = raft
        .wait(Some(timeout))
        .metrics(|m| m.current_leader.is_some(), "have leader")
        .await
        .map_err(|e| ControlPlaneError::Msg(format!("wait leader: {e}")))?;
    m.current_leader
        .ok_or_else(|| ControlPlaneError::Msg("leader vanished".into()))
}

/// Network adapter seam used by tests / harness (not the openraft factory).
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

/// Single-node network: remote RPCs are unreachable (first-binary voter set size 1).
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

/// Factory that resolves peers via live internodes TCP dial of `BasicNode.addr`,
/// falling back to [`PeerRaftRegistry`] for in-process (`inproc://`) harnesses.
#[derive(Clone)]
pub struct PeerNetworkFactory {
    pub registry: Arc<PeerRaftRegistry>,
    pub join_secret: Arc<Vec<u8>>,
}

#[derive(Clone)]
pub struct PeerNetwork {
    target: Uuid,
    addr: String,
    registry: Arc<PeerRaftRegistry>,
    join_secret: Arc<Vec<u8>>,
}

impl RaftNetworkFactory<TypeConfig> for PeerNetworkFactory {
    type Network = PeerNetwork;

    async fn new_client(&mut self, target: Uuid, node: &BasicNode) -> Self::Network {
        PeerNetwork {
            target,
            addr: node.addr.clone(),
            registry: Arc::clone(&self.registry),
            join_secret: Arc::clone(&self.join_secret),
        }
    }
}

/// Adapter that mounts [`PeerRaftRegistry`] onto [`FabricRuntime`] / internodes accept.
pub struct RegistryRaftHandler {
    pub registry: Arc<PeerRaftRegistry>,
    pub local_id: Uuid,
}

/// Mount a [`RegistryRaftHandler`] onto an internode [`spacestorage_internode::FabricRuntime`].
///
/// Production path: inbound `MSG_RAFT_*` frames are dispatched into the local openraft
/// core registered under `local_id`. Outbound RPCs dial `BasicNode.addr` via
/// [`start_raft_peered_with_secret`].
pub fn mount_raft_handler_on_fabric(
    fabric: &spacestorage_internode::FabricRuntime,
    registry: Arc<PeerRaftRegistry>,
    local_id: Uuid,
) {
    fabric.set_raft_handler(Some(Arc::new(RegistryRaftHandler { registry, local_id })));
}

#[async_trait::async_trait]
impl RaftPeerHandler for RegistryRaftHandler {
    async fn handle_raft(&self, kind: RaftRpcKind, payload: &[u8]) -> std::result::Result<Vec<u8>, String> {
        dispatch_peer_rpc(&self.registry, self.local_id, kind, payload)
            .await
            .map_err(|e| e.to_string())
    }
}

fn rpc_unreachable(target: Uuid, detail: impl fmt::Display) -> RPCError<TypeConfig> {
    RPCError::Unreachable(Unreachable::from_string(format!(
        "peer {target} unreachable: {detail}"
    )))
}

fn rpc_network(detail: impl fmt::Display) -> RPCError<TypeConfig> {
    RPCError::Network(NetworkError::from_string(detail.to_string()))
}

/// Prove the internodes wire codec can carry an openraft VoteRequest JSON body.
pub fn encode_vote_wire(rpc: &VoteRequest<TypeConfig>) -> Result<Vec<u8>> {
    encode_raft_payload(RaftRpcKind::Vote, rpc)
        .map_err(|e| ControlPlaneError::Msg(e.to_string()))
}

/// Decode a VoteRequest from an internodes frame payload body.
pub fn decode_vote_wire(payload: &[u8]) -> Result<VoteRequest<TypeConfig>> {
    decode_raft_body(payload).map_err(|e| ControlPlaneError::Msg(e.to_string()))
}

pub fn encode_append_wire(rpc: &AppendEntriesRequest<TypeConfig>) -> Result<Vec<u8>> {
    encode_raft_payload(RaftRpcKind::Append, rpc)
        .map_err(|e| ControlPlaneError::Msg(e.to_string()))
}

pub fn decode_append_wire(payload: &[u8]) -> Result<AppendEntriesRequest<TypeConfig>> {
    decode_raft_body(payload).map_err(|e| ControlPlaneError::Msg(e.to_string()))
}

/// Snapshot wire body: `(vote, meta, data_bytes)`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnapshotWireBody {
    pub vote: VoteOf<TypeConfig>,
    pub meta: openraft::alias::SnapshotMetaOf<TypeConfig>,
    pub data: Vec<u8>,
}

pub fn encode_snapshot_wire(body: &SnapshotWireBody) -> Result<Vec<u8>> {
    encode_raft_payload(RaftRpcKind::Snapshot, body)
        .map_err(|e| ControlPlaneError::Msg(e.to_string()))
}

pub fn decode_snapshot_wire(payload: &[u8]) -> Result<SnapshotWireBody> {
    decode_raft_body(payload).map_err(|e| ControlPlaneError::Msg(e.to_string()))
}

/// Dispatch an inbound internodes Raft peer RPC onto a registered Raft core.
pub async fn dispatch_peer_rpc(
    registry: &PeerRaftRegistry,
    target: Uuid,
    kind: RaftRpcKind,
    payload: &[u8],
) -> Result<Vec<u8>> {
    let raft = registry
        .get(target)
        .ok_or_else(|| ControlPlaneError::Msg(format!("no raft for {target}")))?;
    match kind {
        RaftRpcKind::Vote => {
            let req: VoteRequest<TypeConfig> = decode_vote_wire(payload)?;
            let resp = raft
                .vote(req)
                .await
                .map_err(|e| ControlPlaneError::Msg(format!("vote: {e}")))?;
            serde_json::to_vec(&resp).map_err(|e| ControlPlaneError::Msg(e.to_string()))
        }
        RaftRpcKind::Append => {
            let req: AppendEntriesRequest<TypeConfig> = decode_append_wire(payload)?;
            let resp = raft
                .append_entries(req)
                .await
                .map_err(|e| ControlPlaneError::Msg(format!("append: {e}")))?;
            serde_json::to_vec(&resp).map_err(|e| ControlPlaneError::Msg(e.to_string()))
        }
        RaftRpcKind::Snapshot => {
            let body = decode_snapshot_wire(payload)?;
            let snapshot = Snapshot {
                meta: body.meta,
                snapshot: Cursor::new(body.data),
            };
            let resp = raft
                .install_full_snapshot(body.vote, snapshot)
                .await
                .map_err(|e| ControlPlaneError::Msg(format!("snapshot: {e}")))?;
            serde_json::to_vec(&resp).map_err(|e| ControlPlaneError::Msg(e.to_string()))
        }
    }
}

impl RaftNetworkV2<TypeConfig> for PeerNetwork {
    type SnapshotData = Cursor<Vec<u8>>;

    async fn append_entries(
        &mut self,
        rpc: AppendEntriesRequest<TypeConfig>,
        _option: RPCOption,
    ) -> std::result::Result<AppendEntriesResponse<TypeConfig>, RPCError<TypeConfig>> {
        let body = serde_json::to_vec(&rpc).map_err(|e| rpc_network(e))?;
        let resp_bytes = self.deliver(RaftRpcKind::Append, &body).await?;
        serde_json::from_slice(&resp_bytes).map_err(|e| rpc_network(e))
    }

    async fn vote(
        &mut self,
        rpc: VoteRequest<TypeConfig>,
        _option: RPCOption,
    ) -> std::result::Result<VoteResponse<TypeConfig>, RPCError<TypeConfig>> {
        let body = serde_json::to_vec(&rpc).map_err(|e| rpc_network(e))?;
        let resp_bytes = self.deliver(RaftRpcKind::Vote, &body).await?;
        serde_json::from_slice(&resp_bytes).map_err(|e| rpc_network(e))
    }

    async fn full_snapshot(
        &mut self,
        vote: VoteOf<TypeConfig>,
        snapshot: SnapshotOf<TypeConfig, Self::SnapshotData>,
        cancel: impl Future<Output = ReplicationClosed> + OptionalSend + 'static,
        _option: RPCOption,
    ) -> std::result::Result<SnapshotResponse<TypeConfig>, StreamingError<TypeConfig>> {
        let body = SnapshotWireBody {
            vote: vote.clone(),
            meta: snapshot.meta.clone(),
            data: snapshot.snapshot.into_inner(),
        };
        let payload = serde_json::to_vec(&body)
            .map_err(|e| StreamingError::Network(NetworkError::from_string(e.to_string())))?;

        tokio::pin!(cancel);
        tokio::select! {
            closed = &mut cancel => Err(StreamingError::Closed(closed)),
            res = self.deliver_streaming(RaftRpcKind::Snapshot, &payload) => {
                let resp_bytes = res?;
                serde_json::from_slice(&resp_bytes)
                    .map_err(|e| StreamingError::Network(NetworkError::from_string(e.to_string())))
            }
        }
    }
}

impl PeerNetwork {
    /// Production path: dial `BasicNode.addr` over internodes TCP when possible;
    /// otherwise deliver via in-process registry (inproc harness).
    async fn deliver(
        &self,
        kind: RaftRpcKind,
        body: &[u8],
    ) -> std::result::Result<Vec<u8>, RPCError<TypeConfig>> {
        // Prove length-prefixed internodes framing round-trips (contracts/raft-rpc.md).
        let framed = spacestorage_internode::encode_frame(kind.msg_type(), body);
        let (_k, payload) = spacestorage_internode::decode_raft_frame(&framed)
            .map_err(|e| rpc_network(e))?;

        if let Some(dial) = tcp_dial_addr(&self.addr) {
            return raft_rpc(&dial, self.join_secret.as_slice(), kind, &payload)
                .await
                .map_err(|e| rpc_unreachable(self.target, e));
        }

        // inproc:// harness: deliver through registry after wire decode.
        dispatch_peer_rpc(&self.registry, self.target, kind, &payload)
            .await
            .map_err(|e| rpc_unreachable(self.target, e))
    }

    async fn deliver_streaming(
        &self,
        kind: RaftRpcKind,
        body: &[u8],
    ) -> std::result::Result<Vec<u8>, StreamingError<TypeConfig>> {
        self.deliver(kind, body).await.map_err(|e| match e {
            RPCError::Unreachable(u) => StreamingError::Unreachable(u),
            RPCError::Network(n) => StreamingError::Network(n),
            other => StreamingError::Network(NetworkError::from_string(other.to_string())),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::group::GroupId;
    use crate::raft_net::RaftVote;
    use crate::raft_store::RaftBody;
    use openraft::type_config::async_runtime::watch::WatchReceiver;

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

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn three_node_openraft_election_and_client_write() {
        let registry = Arc::new(PeerRaftRegistry::new());
        let ids: Vec<Uuid> = (0..3).map(|_| Uuid::now_v7()).collect();
        let mut rafts = Vec::new();
        for &id in &ids {
            let r = start_raft_peered(id, Arc::clone(&registry))
                .await
                .expect("Raft::new peered");
            registry.register(id, r.clone());
            rafts.push(r);
        }

        let mut members = BTreeMap::new();
        for &id in &ids {
            members.insert(id, BasicNode::new(format!("inproc://{id}")));
        }
        // Only one node initializes; peers learn via AppendEntries over PeerNetwork.
        bootstrap_multi_voter(&rafts[0], members)
            .await
            .expect("initialize 3 voters");

        // Wait until some node is actually Leader (not just a stale current_leader hint).
        let mut leader_id = None;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while tokio::time::Instant::now() < deadline {
            for (i, r) in rafts.iter().enumerate() {
                if r.is_leader() {
                    leader_id = Some(ids[i]);
                    break;
                }
            }
            if leader_id.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let leader_id = leader_id.expect("a member became Leader");
        assert!(ids.contains(&leader_id), "leader must be a member");

        let leader = registry.get(leader_id).expect("leader handle");
        // Confirm leadership is stable enough for a write.
        leader
            .wait(Some(Duration::from_secs(5)))
            .metrics(|m| m.state == ServerState::Leader, "stable leader")
            .await
            .expect("leader state");

        let req = RaftAppRequest {
            group: GroupId::Cluster,
            body: RaftBody::Noop,
        };
        let resp = leader
            .client_write(req)
            .await
            .expect("client_write on multi-node leader");
        let committed = resp.log_id.index();
        assert!(committed >= 1);

        // Followers catch up via AppendEntries peer RPC.
        for r in &rafts {
            r.wait(Some(Duration::from_secs(10)))
                .metrics(
                    |m| {
                        m.last_applied
                            .as_ref()
                            .map(|id| id.index() >= committed)
                            .unwrap_or(false)
                    },
                    "applied commit",
                )
                .await
                .expect("follower apply");
        }

        let m = leader.metrics().borrow_watched().clone();
        assert_eq!(m.current_leader, Some(leader_id));
    }

    #[tokio::test]
    async fn dispatch_peer_rpc_vote_via_wire() {
        let registry = Arc::new(PeerRaftRegistry::new());
        let id = Uuid::now_v7();
        let raft = start_raft_peered(id, Arc::clone(&registry))
            .await
            .expect("raft");
        registry.register(id, raft.clone());
        bootstrap_single_voter(&raft, id).await.expect("leader");

        // Build a synth vote against the leader (term catch-up path).
        let vote_req = VoteRequest::<TypeConfig>::new(
            openraft::Vote::new(raft.metrics().borrow_watched().current_term + 1, id),
            None,
        );
        let framed = encode_vote_wire(&vote_req).expect("encode");
        let (kind, payload) =
            spacestorage_internode::decode_raft_frame(&framed).expect("frame");
        assert_eq!(kind, RaftRpcKind::Vote);
        let out = dispatch_peer_rpc(&registry, id, kind, &payload)
            .await
            .expect("dispatch");
        assert!(!out.is_empty());
    }

    /// Accept loop: auth + raft dispatch (mirrors node fabric handler).
    async fn serve_one_raft_conn(
        mut stream: tokio::net::TcpStream,
        secret: Arc<Vec<u8>>,
        handler: Arc<RegistryRaftHandler>,
    ) {
        use spacestorage_internode::{decode_frame, encode_frame, registry};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut buf = Vec::new();
        let mut tmp = [0u8; 8192];
        let mut authed = false;
        loop {
            let n = match stream.read(&mut tmp).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            buf.extend_from_slice(&tmp[..n]);
            while let Ok((frame, consumed)) = decode_frame(&buf) {
                buf.drain(..consumed);
                if !authed {
                    if frame.payload.as_slice() != secret.as_slice() {
                        return;
                    }
                    authed = true;
                    let _ = stream
                        .write_all(&encode_frame(registry::MSG_ACK, br#"{"ok":true}"#))
                        .await;
                    continue;
                }
                let Some(kind) = RaftRpcKind::from_msg_type(frame.msg_type) else {
                    let _ = stream
                        .write_all(&encode_frame(
                            registry::MSG_ACK,
                            registry::unknown_ack_payload(),
                        ))
                        .await;
                    continue;
                };
                match handler.handle_raft(kind, &frame.payload).await {
                    Ok(body) => {
                        let _ = stream
                            .write_all(&encode_frame(registry::MSG_ACK, &body))
                            .await;
                    }
                    Err(e) => {
                        let err = format!(r#"{{"ok":false,"error":"{e}"}}"#);
                        let _ = stream
                            .write_all(&encode_frame(registry::MSG_ACK, err.as_bytes()))
                            .await;
                    }
                }
            }
        }
    }

    async fn spawn_tcp_raft_node(
        registry: Arc<PeerRaftRegistry>,
        secret: Arc<Vec<u8>>,
    ) -> (Uuid, String, ControlRaft, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().unwrap();
        let dial = format!("{}:{}", addr.ip(), addr.port());
        let id = Uuid::now_v7();
        let raft = start_raft_peered_with_secret(id, Arc::clone(&registry), Arc::clone(&secret))
            .await
            .expect("raft");
        registry.register(id, raft.clone());
        let handler = Arc::new(RegistryRaftHandler {
            registry: Arc::clone(&registry),
            local_id: id,
        });
        let secret_accept = Arc::clone(&secret);
        let accept = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let h = Arc::clone(&handler);
                let s = Arc::clone(&secret_accept);
                tokio::spawn(async move {
                    serve_one_raft_conn(stream, s, h).await;
                });
            }
        });
        (id, dial, raft, accept)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn three_node_openraft_live_tcp_election_and_client_write() {
        let registry = Arc::new(PeerRaftRegistry::new());
        let secret = Arc::new(b"live-tcp-raft-secret-01234567".to_vec());
        let mut nodes = Vec::new();
        for _ in 0..3 {
            nodes.push(spawn_tcp_raft_node(Arc::clone(&registry), Arc::clone(&secret)).await);
        }

        let mut members = BTreeMap::new();
        for (id, dial, _, _) in &nodes {
            members.insert(*id, BasicNode::new(dial.clone()));
        }
        bootstrap_multi_voter(&nodes[0].2, members)
            .await
            .expect("initialize 3 voters over TCP");

        let mut leader_id = None;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        while tokio::time::Instant::now() < deadline {
            for (id, _, raft, _) in &nodes {
                if raft.is_leader() {
                    leader_id = Some(*id);
                    break;
                }
            }
            if leader_id.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
        let leader_id = leader_id.expect("TCP cluster elected a leader");
        let leader = registry.get(leader_id).expect("leader");
        leader
            .wait(Some(Duration::from_secs(5)))
            .metrics(|m| m.state == ServerState::Leader, "stable")
            .await
            .expect("leader");

        let resp = leader
            .client_write(RaftAppRequest {
                group: GroupId::Cluster,
                body: RaftBody::Noop,
            })
            .await
            .expect("client_write over live TCP");
        let committed = resp.log_id.index();
        assert!(committed >= 1);

        for (_, _, raft, _) in &nodes {
            raft.wait(Some(Duration::from_secs(15))
                )
                .metrics(
                    |m| {
                        m.last_applied
                            .as_ref()
                            .map(|id| id.index() >= committed)
                            .unwrap_or(false)
                    },
                    "applied",
                )
                .await
                .expect("follower apply via TCP");
        }
        for (_, _, _, h) in nodes {
            h.abort();
        }
    }

    /// Handler that can blackhole Raft RPCs (simulated partition / restart window).
    struct GatedRaftHandler {
        inner: RegistryRaftHandler,
        open: Arc<std::sync::atomic::AtomicBool>,
    }

    #[async_trait::async_trait]
    impl RaftPeerHandler for GatedRaftHandler {
        async fn handle_raft(
            &self,
            kind: RaftRpcKind,
            payload: &[u8],
        ) -> std::result::Result<Vec<u8>, String> {
            if !self.open.load(std::sync::atomic::Ordering::SeqCst) {
                // Hang until client RPC timeout — simulates downed accept / restart window.
                tokio::time::sleep(Duration::from_secs(30)).await;
                return Err("blackholed".into());
            }
            self.inner.handle_raft(kind, payload).await
        }
    }

    async fn spawn_tcp_raft_node_gated(
        registry: Arc<PeerRaftRegistry>,
        secret: Arc<Vec<u8>>,
        open: Arc<std::sync::atomic::AtomicBool>,
    ) -> (Uuid, String, ControlRaft, tokio::task::JoinHandle<()>) {
        use spacestorage_internode::{decode_frame, encode_frame, registry as reg};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().unwrap();
        let dial = format!("{}:{}", addr.ip(), addr.port());
        let id = Uuid::now_v7();
        let raft = start_raft_peered_with_secret(id, Arc::clone(&registry), Arc::clone(&secret))
            .await
            .expect("raft");
        registry.register(id, raft.clone());
        let handler = Arc::new(GatedRaftHandler {
            inner: RegistryRaftHandler {
                registry: Arc::clone(&registry),
                local_id: id,
            },
            open,
        });
        let secret_accept = Arc::clone(&secret);
        let accept = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let h = Arc::clone(&handler);
                let s = Arc::clone(&secret_accept);
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut tmp = [0u8; 8192];
                    let mut authed = false;
                    loop {
                        let n = match stream.read(&mut tmp).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => n,
                        };
                        buf.extend_from_slice(&tmp[..n]);
                        while let Ok((frame, consumed)) = decode_frame(&buf) {
                            buf.drain(..consumed);
                            if !authed {
                                if frame.payload.as_slice() != s.as_slice() {
                                    return;
                                }
                                authed = true;
                                let _ = stream
                                    .write_all(&encode_frame(reg::MSG_ACK, br#"{"ok":true}"#))
                                    .await;
                                continue;
                            }
                            let Some(kind) = RaftRpcKind::from_msg_type(frame.msg_type) else {
                                continue;
                            };
                            match h.handle_raft(kind, &frame.payload).await {
                                Ok(body) => {
                                    let _ = stream
                                        .write_all(&encode_frame(reg::MSG_ACK, &body))
                                        .await;
                                }
                                Err(_) => break, // drop conn on blackhole/error
                            }
                        }
                    }
                });
            }
        });
        (id, dial, raft, accept)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn three_node_tcp_follower_restart_catch_up() {
        let registry = Arc::new(PeerRaftRegistry::new());
        let secret = Arc::new(b"restart-raft-secret-012345678".to_vec());
        let gates: Vec<_> = (0..3)
            .map(|_| Arc::new(std::sync::atomic::AtomicBool::new(true)))
            .collect();
        let mut nodes = Vec::new();
        for g in &gates {
            nodes.push(
                spawn_tcp_raft_node_gated(
                    Arc::clone(&registry),
                    Arc::clone(&secret),
                    Arc::clone(g),
                )
                .await,
            );
        }
        let mut members = BTreeMap::new();
        for (id, dial, _, _) in &nodes {
            members.insert(*id, BasicNode::new(dial.clone()));
        }
        bootstrap_multi_voter(&nodes[0].2, members)
            .await
            .expect("init");

        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        let mut leader_idx = 0;
        loop {
            for (i, (_, _, raft, _)) in nodes.iter().enumerate() {
                if raft.is_leader() {
                    leader_idx = i;
                    break;
                }
            }
            if nodes[leader_idx].2.is_leader() {
                break;
            }
            if tokio::time::Instant::now() > deadline {
                panic!("no leader");
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }

        let follower_idx = (0..3).find(|i| *i != leader_idx).unwrap();
        // Simulate follower restart window: blackhole inbound Raft RPCs (same addr).
        gates[follower_idx].store(false, std::sync::atomic::Ordering::SeqCst);

        let leader = nodes[leader_idx].2.clone();
        let resp = leader
            .client_write(RaftAppRequest {
                group: GroupId::Cluster,
                body: RaftBody::Noop,
            })
            .await
            .expect("majority write during follower blackhole");
        let committed = resp.log_id.index();

        // "Restart complete": re-open accept path; follower catches up via AppendEntries.
        gates[follower_idx].store(true, std::sync::atomic::Ordering::SeqCst);

        nodes[follower_idx]
            .2
            .wait(Some(Duration::from_secs(20)))
            .metrics(
                |m| {
                    m.last_applied
                        .as_ref()
                        .map(|id| id.index() >= committed)
                        .unwrap_or(false)
                },
                "restart catch-up",
            )
            .await
            .expect("follower caught up after restart window");

        for (_, _, _, h) in nodes {
            h.abort();
        }
    }

    #[tokio::test]
    async fn snapshot_wire_dispatch_installs() {
        // Practical snapshot catch-up seam: full Snapshot RPC via dispatch_peer_rpc.
        let registry = Arc::new(PeerRaftRegistry::new());
        let id = Uuid::now_v7();
        let raft = start_raft_peered(id, Arc::clone(&registry))
            .await
            .expect("raft");
        registry.register(id, raft.clone());
        bootstrap_single_voter(&raft, id).await.expect("leader");

        let meta = openraft::SnapshotMeta {
            last_log_id: raft.metrics().borrow_watched().last_applied,
            last_membership: Default::default(),
        };
        let body = SnapshotWireBody {
            vote: openraft::Vote::new(
                raft.metrics().borrow_watched().current_term,
                id,
            ),
            meta,
            data: b"snap-bytes".to_vec(),
        };
        let framed = encode_snapshot_wire(&body).expect("encode");
        let (kind, payload) =
            spacestorage_internode::decode_raft_frame(&framed).expect("frame");
        assert_eq!(kind, RaftRpcKind::Snapshot);
        let out = dispatch_peer_rpc(&registry, id, kind, &payload)
            .await
            .expect("snapshot dispatch");
        assert!(!out.is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn three_node_tcp_partition_heals() {
        let registry = Arc::new(PeerRaftRegistry::new());
        let secret = Arc::new(b"partition-raft-secret-0123456".to_vec());
        let mut nodes = Vec::new();
        for _ in 0..3 {
            nodes.push(spawn_tcp_raft_node(Arc::clone(&registry), Arc::clone(&secret)).await);
        }
        let mut members = BTreeMap::new();
        for (id, dial, _, _) in &nodes {
            members.insert(*id, BasicNode::new(dial.clone()));
        }
        bootstrap_multi_voter(&nodes[0].2, members)
            .await
            .expect("init");

        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        while tokio::time::Instant::now() < deadline {
            if nodes.iter().any(|(_, _, r, _)| r.is_leader()) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
        assert!(nodes.iter().any(|(_, _, r, _)| r.is_leader()));

        // Partition: abort accept on one follower (inbound blackhole) and unregister
        // so outbound dials fail → majority of 2 continues.
        let leader_idx = nodes
            .iter()
            .position(|(_, _, r, _)| r.is_leader())
            .expect("leader");
        let part_idx = (0..3).find(|i| *i != leader_idx).unwrap();
        let (pid, _, _, part_accept) = &nodes[part_idx];
        let pid = *pid;
        part_accept.abort();
        // Keep registry entry so leadership doesn't immediately forget the member,
        // but dials to the dead port fail → Unreachable (simulated partition).

        let leader = nodes[leader_idx].2.clone();
        let resp = leader
            .client_write(RaftAppRequest {
                group: GroupId::Cluster,
                body: RaftBody::Noop,
            })
            .await
            .expect("majority write during partition");
        assert!(resp.log_id.index() >= 1);
        let _ = pid;

        for (i, (_, _, _, h)) in nodes.into_iter().enumerate() {
            if i != part_idx {
                h.abort();
            }
        }
    }
}

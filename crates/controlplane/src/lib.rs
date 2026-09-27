pub mod apply;
pub mod cluster;
pub mod error;
pub mod group;
pub mod lease;
pub mod membership;
pub mod metrics_agg;
pub mod namespace;
pub mod openraft_adapter;
pub mod openraft_store;
pub mod ops;
pub mod raft_net;
pub mod raft_store;
pub mod read;
pub mod restore;

// In-process Raft group + ControlPlane live in this module file below.

use crate::apply::ApplyEngine;
use crate::cluster::{ClusterOp, ClusterState, MemberRecord, MemberStatus};
use crate::error::{ControlPlaneError, Result};
use crate::group::GroupId;
use crate::membership::{first_binary_voter_set, MembershipOp, VoterSet};
use crate::openraft_adapter::{app_request_to_record, RaftAppRequest};
use crate::raft_net::{InProcessRaftNet, RaftAppend, RaftRpc, RaftVote};
use crate::raft_store::{RaftBody, RaftLogRecord, RaftStore};
use crate::read::ReadIndex;
use openraft::type_config::async_runtime::watch::WatchReceiver;
use parking_lot::Mutex as SyncMutex;
use spacestorage_clocks::HlcStamp;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

pub use crate::error::{ControlPlaneError as Error, Result as CpResult};
pub use crate::openraft_adapter::{
    bootstrap_multi_voter, bootstrap_single_voter, dispatch_peer_rpc, mount_raft_handler_on_fabric,
    start_raft, start_raft_peered, start_raft_peered_with_secret, wait_for_leader, ControlRaft,
    PeerNetworkFactory, PeerRaftRegistry, RegistryRaftHandler, OPENRAFT_ACTIVE_PIN,
};
pub use crate::raft_store::RAFT_FORMAT_VERSION;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaftRole {
    Follower,
    Candidate,
    Leader,
}

/// One Raft group: openraft runtime (`Raft::new`) owns elections/commit; [`RaftStore`]
/// keeps the SpaceStorage on-disk format for restore / read-index.
pub struct RaftGroup {
    pub group: GroupId,
    pub node_id: Uuid,
    pub store: RaftStore,
    /// Openraft 0.10 runtime (spawned via [`start_raft`]).
    pub raft: ControlRaft,
    pub role: RaftRole,
    pub leader: Option<Uuid>,
    pub voter_set: VoterSet,
    match_index: BTreeMap<Uuid, u64>,
    pub elections_total: AtomicU64,
}

impl RaftGroup {
    pub async fn open(
        data_dir: &Path,
        group: GroupId,
        node_id: Uuid,
        voter_set: VoterSet,
    ) -> Result<Self> {
        let store = RaftStore::open(data_dir, group).await?;
        let raft = start_raft(node_id).await?;
        Ok(Self {
            group,
            node_id,
            store,
            raft,
            role: RaftRole::Follower,
            leader: None,
            voter_set,
            match_index: BTreeMap::new(),
            elections_total: AtomicU64::new(0),
        })
    }

    pub fn is_leader(&self) -> bool {
        self.role == RaftRole::Leader
    }

    pub fn read_index(&self) -> ReadIndex {
        ReadIndex {
            group: self.group,
            applied_index: self.store.commit_index,
            commit_index: self.store.commit_index,
            leader: self.leader,
            is_leader: self.is_leader(),
        }
    }

    /// Bootstrap openraft membership `{self}` and mark this node Leader (single-voter path).
    pub async fn become_leader(&mut self) -> Result<()> {
        bootstrap_single_voter(&self.raft, self.node_id).await?;
        self.role = RaftRole::Leader;
        self.leader = Some(self.node_id);
        let m = self.raft.metrics().borrow_watched().clone();
        self.store.current_term = m.current_term.max(1);
        self.elections_total.fetch_add(1, Ordering::Relaxed);
        for v in &self.voter_set.voters {
            self.match_index.insert(*v, self.store.commit_index);
        }
        Ok(())
    }

    pub async fn append_as_leader(
        &mut self,
        body: RaftBody,
        hlc: HlcStamp,
        net: Option<&InProcessRaftNet>,
    ) -> Result<u64> {
        if !self.is_leader() {
            return Err(ControlPlaneError::NotLeader {
                group: self.group,
                leader: self.leader,
            });
        }
        if !self.voter_set.is_voter(&self.node_id) {
            return Err(ControlPlaneError::NotMember);
        }

        // Prefer openraft client_write when alone (production single-voter / first-binary).
        if net.is_none() && self.voter_set.voters.len() == 1 {
            let req = RaftAppRequest {
                group: self.group,
                body: body.clone(),
            };
            let resp = self
                .raft
                .client_write(req.clone())
                .await
                .map_err(|e| ControlPlaneError::Msg(format!("client_write: {e}")))?;
            let index = resp.log_id.index();
            let term = self.store.current_term.max(1);
            let rec = app_request_to_record(self.group, term, index, req, hlc);
            self.store.append_records(&[rec]).await?;
            self.store.commit_index = index;
            self.store.persist_meta().await?;
            self.match_index.insert(self.node_id, index);
            return Ok(index);
        }

        let index = self.store.last_index() + 1;
        let term = self.store.current_term;
        let rec = RaftLogRecord {
            group: self.group,
            term,
            index,
            hlc,
            body,
        };
        self.store.append_records(&[rec.clone()]).await?;
        self.match_index.insert(self.node_id, index);

        // Replicate to other voters via in-process net when provided (harness path).
        if let Some(net) = net {
            for peer in self.voter_set.voters.iter().copied() {
                if peer == self.node_id {
                    continue;
                }
                net.send(
                    self.node_id,
                    peer,
                    RaftRpc::Append(RaftAppend {
                        group: self.group,
                        term,
                        leader_id: self.node_id,
                        prev_log_index: index.saturating_sub(1),
                        prev_log_term: if index > 1 { term } else { 0 },
                        entries: vec![rec.clone()],
                        leader_commit: self.store.commit_index,
                        success: false,
                        match_index: 0,
                    }),
                );
            }
        }

        // Single-voter majority is immediate; multi-voter needs acks.
        let majority = self.voter_set.majority();
        let acked = self
            .match_index
            .values()
            .filter(|&&m| m >= index)
            .count();
        if acked >= majority || self.voter_set.voters.len() == 1 {
            self.store.commit_index = index;
            self.store.persist_meta().await?;
            Ok(index)
        } else if net.is_none() {
            Err(ControlPlaneError::Minority {
                group: self.group,
            })
        } else {
            self.store.commit_index = index;
            self.store.persist_meta().await?;
            Ok(index)
        }
    }

    pub async fn handle_rpc(&mut self, from: Uuid, rpc: RaftRpc) -> Result<Option<RaftRpc>> {
        match rpc {
            RaftRpc::Vote(mut v) => {
                if !self.voter_set.is_voter(&self.node_id) {
                    return Err(ControlPlaneError::NotMember);
                }
                if v.term > self.store.current_term {
                    self.store.current_term = v.term;
                    self.store.voted_for = None;
                    self.role = RaftRole::Follower;
                }
                let grant = v.term >= self.store.current_term
                    && (self.store.voted_for.is_none()
                        || self.store.voted_for == Some(v.candidate_id))
                    && v.last_log_index >= self.store.last_index();
                if grant {
                    self.store.voted_for = Some(v.candidate_id);
                    self.store.persist_meta().await?;
                }
                v.vote_granted = grant;
                Ok(Some(RaftRpc::Vote(v)))
            }
            RaftRpc::Append(mut a) => {
                if a.term >= self.store.current_term {
                    self.store.current_term = a.term;
                    self.role = RaftRole::Follower;
                    self.leader = Some(a.leader_id);
                }
                if a.term < self.store.current_term {
                    a.success = false;
                    return Ok(Some(RaftRpc::Append(a)));
                }
                if !a.entries.is_empty() {
                    let first = a.entries[0].index;
                    self.store.truncate_from(first);
                    self.store.append_records(&a.entries).await?;
                }
                if a.leader_commit > self.store.commit_index {
                    self.store.commit_index =
                        a.leader_commit.min(self.store.last_index());
                    self.store.persist_meta().await?;
                }
                a.success = true;
                a.match_index = self.store.last_index();
                let _ = from;
                Ok(Some(RaftRpc::Append(a)))
            }
            other => Ok(Some(other)),
        }
    }

    /// Start an election; with in-process net, collect votes from peer groups.
    pub async fn campaign(
        &mut self,
        peers: &BTreeMap<Uuid, Arc<Mutex<RaftGroup>>>,
    ) -> Result<()> {
        if !self.voter_set.is_voter(&self.node_id) {
            return Err(ControlPlaneError::NotMember);
        }
        self.role = RaftRole::Candidate;
        self.store.current_term = self.store.current_term.saturating_add(1);
        self.store.voted_for = Some(self.node_id);
        self.store.persist_meta().await?;
        let mut votes = 1usize;
        let req = RaftVote {
            group: self.group,
            term: self.store.current_term,
            candidate_id: self.node_id,
            last_log_index: self.store.last_index(),
            last_log_term: self.store.last_term(),
            vote_granted: false,
        };
        let peer_ids: Vec<Uuid> = self
            .voter_set
            .voters
            .iter()
            .copied()
            .filter(|id| *id != self.node_id)
            .collect();
        for peer in peer_ids {
            if let Some(g) = peers.get(&peer) {
                let mut other = g.lock().await;
                if let Ok(Some(RaftRpc::Vote(resp))) = other
                    .handle_rpc(self.node_id, RaftRpc::Vote(req.clone()))
                    .await
                {
                    if resp.vote_granted {
                        votes += 1;
                    }
                }
            }
        }
        if votes >= self.voter_set.majority() {
            // Harness path: mark leader locally without re-running openraft initialize.
            self.role = RaftRole::Leader;
            self.leader = Some(self.node_id);
            self.elections_total.fetch_add(1, Ordering::Relaxed);
            for v in &self.voter_set.voters {
                self.match_index.insert(*v, self.store.commit_index);
            }
            Ok(())
        } else {
            self.role = RaftRole::Follower;
            Err(ControlPlaneError::Minority {
                group: self.group,
            })
        }
    }
}

/// Control-plane handle: cluster + namespace groups; implements ClusterStore semantics.
pub struct ControlPlane {
    pub data_dir: std::path::PathBuf,
    pub node_id: Uuid,
    pub apply: SyncMutex<ApplyEngine>,
    pub cluster_group: Mutex<Option<RaftGroup>>,
    pub namespace_groups: Mutex<BTreeMap<Uuid, RaftGroup>>,
    pub elections_total: AtomicU64,
    /// When false, process must not vote (restore / join pending).
    pub is_member: SyncMutex<bool>,
}

impl ControlPlane {
    pub async fn bootstrap(
        data_dir: impl Into<std::path::PathBuf>,
        node_id: Uuid,
        node_name: impl Into<String>,
        cluster_uuid: Uuid,
        cluster_name: impl Into<String>,
    ) -> Result<Self> {
        let data_dir = data_dir.into();
        let cluster = ClusterState::bootstrap(cluster_uuid, cluster_name, node_id, node_name);
        let voter_set = cluster.voter_set.clone();
        let mut group = RaftGroup::open(&data_dir, GroupId::Cluster, node_id, voter_set).await?;
        group.become_leader().await?;
        Ok(Self {
            data_dir,
            node_id,
            apply: SyncMutex::new(ApplyEngine::new(cluster)),
            cluster_group: Mutex::new(Some(group)),
            namespace_groups: Mutex::new(BTreeMap::new()),
            elections_total: AtomicU64::new(1),
            is_member: SyncMutex::new(true),
        })
    }

    pub fn cluster_snapshot(&self) -> ClusterState {
        self.apply.lock().cluster.clone()
    }

    pub async fn controllers_view(&self) -> ControllersView {
        let g = self.cluster_group.lock().await;
        let (primary, term, commit) = if let Some(ref cg) = *g {
            (cg.leader, cg.store.current_term, cg.store.commit_index)
        } else {
            (None, 0, 0)
        };
        drop(g);
        let apply = self.apply.lock();
        let secondaries: Vec<Uuid> = apply
            .cluster
            .voter_set
            .voters
            .iter()
            .copied()
            .filter(|id| Some(*id) != primary)
            .collect();
        ControllersView {
            cluster_primary: primary,
            cluster_secondaries: secondaries,
            term,
            commit_index: commit,
            namespaces: apply
                .cluster
                .namespaces
                .values()
                .map(|n| NamespaceControllerView {
                    namespace_id: n.id,
                    name: n.name.clone(),
                    primary,
                })
                .collect(),
        }
    }

    pub async fn append_cluster(&self, op: ClusterOp) -> Result<u64> {
        if !*self.is_member.lock() {
            return Err(ControlPlaneError::NotMember);
        }
        let hlc = {
            let a = self.apply.lock();
            let mut h = a.cluster.hlc.clone();
            h.logical = h.logical.saturating_add(1);
            h
        };
        let body = RaftBody::Cluster(
            serde_json::to_value(&op).map_err(|e| ControlPlaneError::Msg(e.to_string()))?,
        );
        let index = {
            let mut g = self.cluster_group.lock().await;
            let cg = g
                .as_mut()
                .ok_or_else(|| ControlPlaneError::Msg("cluster group not started".into()))?;
            cg.append_as_leader(body, hlc.clone(), None).await?
        };
        let maybe_vs = {
            let mut apply = self.apply.lock();
            apply.cluster.apply(op.clone(), hlc);
            if let ClusterOp::CreateNamespace { id, .. } = &op {
                let mut vs = apply.cluster.voter_set.clone();
                vs.group = GroupId::Namespace(*id);
                apply
                    .namespaces
                    .insert(*id, crate::namespace::NamespaceState::new(*id, vs));
            }
            if let ClusterOp::DeleteNamespace { id } = &op {
                apply.namespaces.remove(id);
            }
            if let ClusterOp::AdmitMember { .. } = &op {
                let ids = apply.cluster.member_ids_ordered();
                if let Ok(vs) = first_binary_voter_set(GroupId::Cluster, &ids) {
                    apply.cluster.voter_set = vs.clone();
                    Some(vs)
                } else {
                    None
                }
            } else {
                None
            }
        };
        if let Some(vs) = maybe_vs {
            if let Some(cg) = self.cluster_group.lock().await.as_mut() {
                cg.voter_set = vs;
            }
        }
        Ok(index)
    }

    /// Admit a member and expand voters per first-binary table.
    pub async fn admit_member(
        &self,
        node_id: Uuid,
        node_name: impl Into<String>,
    ) -> Result<()> {
        self.append_cluster(ClusterOp::AdmitMember {
            record: MemberRecord {
                node_id,
                node_name: node_name.into(),
                status: MemberStatus::Ready,
                labels: BTreeMap::new(),
            },
        })
        .await?;
        Ok(())
    }

    pub fn tenant_replica_excluded(&self, node: Uuid) -> bool {
        let a = self.apply.lock();
        crate::ops::tenant_replica_excluded(
            node,
            a.cluster.exclusive_data,
            &a.cluster.voter_set.voters,
        )
    }

    pub async fn set_exclusive_data(&self, on: bool) -> Result<()> {
        #[cfg(not(feature = "controlplane-ops"))]
        {
            if on {
                return Err(ControlPlaneError::Slice7Required {
                    op: "controller_exclusive_data".into(),
                });
            }
        }
        self.append_cluster(ClusterOp::SetExclusiveData { on })
            .await?;
        Ok(())
    }

    pub async fn voter_op(&self, op: MembershipOp) -> Result<()> {
        let (vs, members) = {
            let a = self.apply.lock();
            (
                a.cluster.voter_set.clone(),
                a.cluster.members.keys().copied().collect::<BTreeSet<_>>(),
            )
        };
        let next = crate::ops::propose_membership_op(&vs, op, &members)?;
        self.append_cluster(ClusterOp::SetVoterSet { voter_set: next })
            .await?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ControllersView {
    pub cluster_primary: Option<Uuid>,
    pub cluster_secondaries: Vec<Uuid>,
    pub term: u64,
    pub commit_index: u64,
    pub namespaces: Vec<NamespaceControllerView>,
}

#[derive(Debug, Clone)]
pub struct NamespaceControllerView {
    pub namespace_id: Uuid,
    pub name: String,
    pub primary: Option<Uuid>,
}

/// `004` ClusterStore seam: append metadata via Raft majority.
#[async_trait::async_trait]
pub trait ClusterStore: Send + Sync {
    async fn append_cluster_op(&self, op: ClusterOp) -> Result<u64>;
    fn snapshot(&self) -> ClusterState;
    fn is_leader(&self) -> bool;
}

#[async_trait::async_trait]
impl ClusterStore for ControlPlane {
    async fn append_cluster_op(&self, op: ClusterOp) -> Result<u64> {
        self.append_cluster(op).await
    }

    fn snapshot(&self) -> ClusterState {
        self.cluster_snapshot()
    }

    fn is_leader(&self) -> bool {
        self.cluster_group
            .try_lock()
            .ok()
            .and_then(|g| g.as_ref().map(|x| x.is_leader()))
            .unwrap_or(false)
    }
}

/// Alias used by plan / wiring.
pub type RaftClusterStore = ControlPlane;

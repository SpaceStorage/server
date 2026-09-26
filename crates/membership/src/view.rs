//! In-memory membership view (cluster log apply seam until 006 Raft).

use crate::events::MemberStatus;
use crate::token::JoinToken;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemberRecord {
    pub node_id: Uuid,
    pub node_name: String,
    pub status: MemberStatus,
    pub incarnation: u64,
    pub quorum_domain: String,
    pub voter: bool,
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingJoin {
    pub node_id: Uuid,
    pub node_name: String,
    pub labels: BTreeMap<String, String>,
    pub internodes_address: String,
    pub quorum_domain: String,
    pub token_id: Option<Uuid>,
    pub connected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MembershipView {
    pub cluster_uuid: Uuid,
    pub cluster_name: String,
    pub members: BTreeMap<Uuid, MemberRecord>,
    pub pending: BTreeMap<Uuid, PendingJoin>,
    pub retired: BTreeSet<Uuid>,
    pub tokens: BTreeMap<Uuid, JoinToken>,
    pub secret_epoch: u64,
    pub quorum_domain_default: String,
    /// Registered QuorumDomain names (T074 / FR-005).
    #[serde(default)]
    pub quorum_domains: BTreeSet<String>,
}

impl MembershipView {
    pub fn empty(cluster_uuid: Uuid, cluster_name: impl Into<String>, secret_epoch: u64) -> Self {
        Self {
            cluster_uuid,
            cluster_name: cluster_name.into(),
            members: BTreeMap::new(),
            pending: BTreeMap::new(),
            retired: BTreeSet::new(),
            tokens: BTreeMap::new(),
            secret_epoch,
            quorum_domain_default: "default".into(),
            quorum_domains: BTreeSet::new(),
        }
    }

    pub fn domain_exists(&self, name: &str) -> bool {
        self.quorum_domains.contains(name)
    }

    /// Register a quorum domain (bootstrap / CLUSTER_ADMIN `domain-create`).
    pub fn ensure_domain(&mut self, name: impl Into<String>) {
        self.quorum_domains.insert(name.into());
    }

    pub fn is_member(&self, node_id: &Uuid) -> bool {
        self.members.contains_key(node_id)
    }

    pub fn is_pending(&self, node_id: &Uuid) -> bool {
        self.pending.contains_key(node_id)
    }

    /// Voters / replica targets exclude pending and non-members.
    pub fn replica_targets(&self) -> Vec<&MemberRecord> {
        self.members
            .values()
            .filter(|m| matches!(m.status, MemberStatus::Ready))
            .collect()
    }

    pub fn voting_members(&self) -> Vec<&MemberRecord> {
        self.members.values().filter(|m| m.voter).collect()
    }

    /// Adjust voter set per research R12: 1→ keep 1; at 3 members expand to 3 voters;
    /// further members are learners; 3→2 leaves one voter + one learner.
    pub fn recompute_voters(&mut self) {
        let n = self.members.len();
        let ids: Vec<Uuid> = self.members.keys().copied().collect();
        for id in &ids {
            if let Some(m) = self.members.get_mut(id) {
                m.voter = match n {
                    0 => false,
                    1 | 2 => {
                        // Exactly one voter when 1 or 2 members (odd size preserved).
                        ids.iter().position(|x| x == id) == Some(0)
                    }
                    3 => true,
                    _ => ids.iter().position(|x| x == id).map(|i| i < 3).unwrap_or(false),
                };
            }
        }
    }
}

//! Cluster Raft apply machine (membership, namespace list, roles blob, exclusive-data).

use crate::group::GroupId;
use crate::membership::VoterSet;
use serde::{Deserialize, Serialize};
use spacestorage_clocks::HlcStamp;
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemberStatus {
    Joining,
    Ready,
    Draining,
    Dead,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemberRecord {
    pub node_id: Uuid,
    pub node_name: String,
    pub status: MemberStatus,
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NamespaceListEntry {
    pub id: Uuid,
    pub name: String,
    pub group_id: GroupId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterState {
    pub cluster_uuid: Uuid,
    pub cluster_name: String,
    pub members: BTreeMap<Uuid, MemberRecord>,
    pub namespaces: BTreeMap<Uuid, NamespaceListEntry>,
    /// Opaque roles blob until tenancy (007) types replace it.
    pub roles: serde_json::Value,
    pub voter_set: VoterSet,
    pub exclusive_data: bool,
    pub hlc: HlcStamp,
}

impl ClusterState {
    pub fn bootstrap(cluster_uuid: Uuid, cluster_name: impl Into<String>, self_id: Uuid, self_name: impl Into<String>) -> Self {
        let mut members = BTreeMap::new();
        members.insert(
            self_id,
            MemberRecord {
                node_id: self_id,
                node_name: self_name.into(),
                status: MemberStatus::Ready,
                labels: BTreeMap::new(),
            },
        );
        let voter_set = VoterSet::singleton(GroupId::Cluster, self_id).expect("singleton odd");
        Self {
            cluster_uuid,
            cluster_name: cluster_name.into(),
            members,
            namespaces: BTreeMap::new(),
            roles: serde_json::json!({}),
            voter_set,
            exclusive_data: false,
            hlc: HlcStamp {
                domain_id: "cluster".into(),
                physical_micros: 0,
                logical: 0,
                node_id: self_id,
            },
        }
    }

    pub fn member_ids_ordered(&self) -> Vec<Uuid> {
        let mut ids: Vec<Uuid> = self.members.keys().copied().collect();
        ids.sort();
        ids
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ClusterOp {
    AdmitMember { record: MemberRecord },
    CreateNamespace { id: Uuid, name: String },
    RenameNamespace { id: Uuid, new_name: String },
    DeleteNamespace { id: Uuid },
    SetRoles { roles: serde_json::Value },
    SetExclusiveData { on: bool },
    SetVoterSet { voter_set: VoterSet },
}

impl ClusterState {
    pub fn apply(&mut self, op: ClusterOp, hlc: HlcStamp) {
        match op {
            ClusterOp::AdmitMember { record } => {
                self.members.insert(record.node_id, record);
            }
            ClusterOp::CreateNamespace { id, name } => {
                self.namespaces.insert(
                    id,
                    NamespaceListEntry {
                        id,
                        name,
                        group_id: GroupId::Namespace(id),
                    },
                );
            }
            ClusterOp::RenameNamespace { id, new_name } => {
                if let Some(e) = self.namespaces.get_mut(&id) {
                    e.name = new_name;
                }
            }
            ClusterOp::DeleteNamespace { id } => {
                self.namespaces.remove(&id);
            }
            ClusterOp::SetRoles { roles } => {
                self.roles = roles;
            }
            ClusterOp::SetExclusiveData { on } => {
                self.exclusive_data = on;
            }
            ClusterOp::SetVoterSet { voter_set } => {
                self.voter_set = voter_set;
            }
        }
        self.hlc = hlc;
    }
}

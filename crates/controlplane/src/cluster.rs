//! Cluster Raft apply machine (membership, namespace list, roles blob, exclusive-data).
//!
//! ## Kafka ingest rows (T073 / 009)
//!
//! [`ClusterState::kafka_ingests`] and [`ClusterOp::UpsertKafkaIngest`] /
//! [`ClusterOp::DeleteKafkaIngest`] are an **additive cluster-log payload**:
//! ingest declarations are cluster-scoped live objects (009), not namespace
//! schemas/definitions (which MUST stay on the namespace Raft group per FR-003).
//! Keeping them on the cluster membership store preserves live 009 apply on every
//! ready node without inventing a parallel catalog. They are not 006 membership
//! or namespace-store content beyond that additive payload.
//!
//! ## Placement replica health (004 T099)
//!
//! [`ClusterOp::PlacementEvent`] carries `004` [`spacestorage_placement::PlacementEvent`]
//! (today: `ReplicaHealth`). It updates [`ClusterState::replica_describe`] so every
//! node resolves the same health/lag. Schemas and container definitions MUST NOT
//! appear here (FR-003 / data-model §17).
//!
//! ## Data jobs (010 T061)
//!
//! [`ClusterState::data_jobs`] / [`ClusterOp::UpsertDataJob`] hold `JobRecord` JSON for
//! cluster-scoped migrate/backup jobs (cross-namespace, placement, multi-ns backup).
//! Same-namespace transform/copy MAY instead append to the namespace Raft group; the
//! in-memory `JobStore` hydrates from both. Never schemas/definitions here.

use crate::group::GroupId;
use crate::membership::VoterSet;
use serde::{Deserialize, Serialize};
use spacestorage_clocks::HlcStamp;
use spacestorage_placement::{apply_placement_event, DescribeSnapshot, PlacementEvent};
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

/// Member-published drive inventory (FR-003); schemas/definitions stay off this log.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DriveInventory {
    pub drive_id: String,
    pub path: String,
    pub bytes_total: u64,
    pub bytes_free: u64,
}

/// Member-published memory inventory pointer (FR-003).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct MemoryInventory {
    pub bytes_total: u64,
    pub bytes_available: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterState {
    pub cluster_uuid: Uuid,
    pub cluster_name: String,
    pub members: BTreeMap<Uuid, MemberRecord>,
    pub namespaces: BTreeMap<Uuid, NamespaceListEntry>,
    /// Opaque roles blob until tenancy (007) types replace it.
    pub roles: serde_json::Value,
    /// Additive cluster-log payload for 009 Kafka ingest (see module docs / T073).
    #[serde(default)]
    pub kafka_ingests: BTreeMap<Uuid, serde_json::Value>,
    /// Additive cluster-log payload for 010 data_* JobRecord JSON (T061).
    #[serde(default)]
    pub data_jobs: BTreeMap<Uuid, serde_json::Value>,
    /// Per-member drive inventory published into the cluster log (FR-003 / T065).
    #[serde(default)]
    pub drives: BTreeMap<Uuid, Vec<DriveInventory>>,
    /// Per-member memory inventory (FR-003 / T065).
    #[serde(default)]
    pub memory: BTreeMap<Uuid, MemoryInventory>,
    /// Applied `004` replica describe (health + lag); never schemas/definitions.
    #[serde(default)]
    pub replica_describe: DescribeSnapshot,
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
            kafka_ingests: BTreeMap::new(),
            data_jobs: BTreeMap::new(),
            drives: BTreeMap::new(),
            memory: BTreeMap::new(),
            replica_describe: DescribeSnapshot::default(),
            voter_set,
            exclusive_data: false,
            hlc: HlcStamp {
                domain_id: "cluster".into(),
                physical_micros: spacestorage_clocks::stamp::wall_micros(),
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
    /// Publish member drive inventory (FR-003); never carries schemas/definitions.
    PublishDrives {
        node_id: Uuid,
        drives: Vec<DriveInventory>,
    },
    /// Publish member memory inventory (FR-003).
    PublishMemory {
        node_id: Uuid,
        memory: MemoryInventory,
    },
    /// Upsert Kafka ingest declaration (009) into the cluster log (additive T073).
    UpsertKafkaIngest { decl: serde_json::Value },
    /// Delete Kafka ingest by id (009).
    DeleteKafkaIngest { id: Uuid },
    /// Upsert 010 data_* JobRecord (cluster-scoped jobs; T061).
    UpsertDataJob { job: serde_json::Value },
    /// Delete a finished/cancelled data job from the cluster log.
    DeleteDataJob { id: Uuid },
    /// `004` placement event (`ReplicaHealth`, …). Never schemas or container definitions.
    PlacementEvent { event: PlacementEvent },
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
            ClusterOp::PublishDrives { node_id, drives } => {
                self.drives.insert(node_id, drives);
            }
            ClusterOp::PublishMemory { node_id, memory } => {
                self.memory.insert(node_id, memory);
            }
            ClusterOp::UpsertKafkaIngest { decl } => {
                let id = decl
                    .get("id")
                    .and_then(|v| match v {
                        serde_json::Value::String(s) => Uuid::parse_str(s).ok(),
                        other => serde_json::from_value::<Uuid>(other.clone()).ok(),
                    });
                if let Some(id) = id {
                    self.kafka_ingests.insert(id, decl);
                }
            }
            ClusterOp::DeleteKafkaIngest { id } => {
                self.kafka_ingests.remove(&id);
            }
            ClusterOp::UpsertDataJob { job } => {
                let id = job
                    .get("id")
                    .and_then(|v| match v {
                        serde_json::Value::String(s) => Uuid::parse_str(s).ok(),
                        other => serde_json::from_value::<Uuid>(other.clone()).ok(),
                    });
                if let Some(id) = id {
                    self.data_jobs.insert(id, job);
                }
            }
            ClusterOp::DeleteDataJob { id } => {
                self.data_jobs.remove(&id);
            }
            ClusterOp::PlacementEvent { event } => {
                apply_placement_event(&mut self.replica_describe, &event);
            }
        }
        self.hlc = hlc;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spacestorage_placement::{ReplicaHealth, ReplicaLag};
    use std::time::Duration;

    #[test]
    fn placement_replica_health_updates_describe_not_schemas() {
        let self_id = Uuid::nil();
        let mut st = ClusterState::bootstrap(Uuid::nil(), "c", self_id, "n1");
        let event = PlacementEvent::ReplicaHealth {
            id: Uuid::nil(),
            origin: "n1".into(),
            namespace: "ns".into(),
            container: "kv".into(),
            node_name: "n2".into(),
            health: ReplicaHealth::Behind,
            lag: ReplicaLag::new(Duration::from_millis(3210), 4),
            replica_id: None,
        };
        st.apply(
            ClusterOp::PlacementEvent { event },
            HlcStamp {
                domain_id: "cluster".into(),
                physical_micros: 1,
                logical: 0,
                node_id: self_id,
            },
        );
        let rows = st.replica_describe.describe_container("ns", "kv");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].lag.duration_ms, 3210);
        assert_eq!(rows[0].health, ReplicaHealth::Behind);
        // Cluster log still has no schema/definition maps — only describe rows.
        assert!(st.namespaces.is_empty());
    }
}

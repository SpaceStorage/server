use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Cluster-scoped membership events (011). This crate is the only writer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MembershipEvent {
    Bootstrap {
        cluster_uuid: Uuid,
        cluster_name: String,
        node_id: Uuid,
        node_name: String,
        quorum_domain: String,
    },
    PendingJoin {
        node_id: Uuid,
        node_name: String,
        labels: Vec<(String, String)>,
        internodes_address: String,
        token_id: Option<Uuid>,
    },
    AdmitMember {
        node_id: Uuid,
        node_name: String,
        quorum_domain: String,
        incarnation: u64,
    },
    MemberUpdate {
        node_id: Uuid,
        status: MemberStatus,
    },
    Drain {
        node_id: Uuid,
    },
    Undrain {
        node_id: Uuid,
    },
    RemoveMember {
        node_id: Uuid,
    },
    RetireIdentity {
        node_id: Uuid,
        former_name: String,
    },
    ReplaceMember {
        node_id: Uuid,
        incarnation: u64,
    },
    SecretRotateBegin {
        epoch: u64,
    },
    SecretRotateComplete {
        epoch: u64,
    },
    JoinTokenMint {
        token_id: Uuid,
        node_name: String,
        node_id: Option<Uuid>,
    },
    JoinTokenConsume {
        token_id: Uuid,
        node_id: Uuid,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemberStatus {
    Ready,
    Draining,
}

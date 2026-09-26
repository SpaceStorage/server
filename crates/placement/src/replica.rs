//! Replica health states (FB subset).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReplicaHealth {
    Empty,
    InSync,
    Behind,
    Unavailable,
    Moving,
    Removed,
    ContentLost,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Replica {
    pub node_name: String,
    pub health: ReplicaHealth,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReplicaSet {
    pub replicas: Vec<Replica>,
}

//! Coordinator RPC types for leaderless FanoutWrite/Read (004 FR-045).
//!
//! Semantics (in-domain durable wait) live in `spacestorage-placement::fanout`.
//! This module is the internode wire shape only — no Raft / preferred leader.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Best-effort, additive tenancy usage propagation. This is never a Raft payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuotaDelta {
    pub namespace: Uuid,
    pub unit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub datatype: Option<String>,
    pub delta: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanoutWrite {
    pub container_id: String,
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub stamp_physical: u64,
    pub stamp_logical: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanoutRead {
    pub container_id: String,
    pub key: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcAck {
    pub ok: bool,
    pub error: Option<String>,
    /// Crash-durable only after spawn_blocking fsync on the accept path.
    pub durable: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_delta_roundtrips_as_additive_wire_type() {
        let delta = QuotaDelta {
            namespace: Uuid::from_u128(7),
            unit: "bytes".into(),
            datatype: Some("kv_store".into()),
            delta: -3,
        };
        let bytes = serde_json::to_vec(&delta).unwrap();
        assert_eq!(serde_json::from_slice::<QuotaDelta>(&bytes).unwrap(), delta);
    }
}

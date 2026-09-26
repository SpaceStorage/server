//! Coordinator RPC types for leaderless FanoutWrite/Read (004 FR-045).
//!
//! Semantics (in-domain durable wait) live in `spacestorage-placement::fanout`.
//! This module is the internode wire shape only — no Raft / preferred leader.

use serde::{Deserialize, Serialize};

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

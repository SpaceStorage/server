//! Execution errors (005 contracts).

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Named part that could not meet quorum / had zero live replicas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartUnavailable {
    pub container: String,
    pub shard: Option<String>,
    pub partition: Option<String>,
    pub live_replicas: u16,
    pub required_level: String,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ExecError {
    #[error("timeout")]
    Timeout,
    #[error("cancelled")]
    Cancelled,
    #[error("unavailable: {0:?}")]
    Unavailable(PartUnavailable),
    #[error("not_supported: {0}")]
    NotSupported(String),
    #[error("admission_rejected{{limit:{limit},current:{current},max:{max}}}")]
    Admission {
        limit: String,
        current: u64,
        max: u64,
    },
    #[error("snapshot_unsupported{{type:{0}}}")]
    SnapshotUnsupported(String),
    #[error("isolation: {0}")]
    Isolation(String),
    #[error("quorum_unsatisfiable")]
    QuorumUnsatisfiable,
    #[error("retryable_txn: {0}")]
    RetryableTxn(String),
    #[error("stage_failed{{stage:{0}}}")]
    StageFailed(String),
    #[error("limit_exceeded{{what:{0}}}")]
    LimitExceeded(String),
    #[error("unsupported_by_type{{container:{container},type:{ty},op:{op}}}")]
    UnsupportedByType {
        container: String,
        ty: String,
        op: String,
    },
    #[error("cyclic_composition")]
    CyclicComposition,
    #[error("{0}")]
    Msg(String),
}

impl ExecError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::Unavailable(_) => "part_unavailable",
            Self::NotSupported(_) => "not_supported",
            Self::Admission { .. } => "admission_rejected",
            Self::SnapshotUnsupported(_) => "snapshot_unsupported",
            Self::Isolation(_) => "isolation",
            Self::QuorumUnsatisfiable => "quorum_unsatisfiable",
            Self::RetryableTxn(_) => "retryable_txn",
            Self::StageFailed(_) => "stage_failed",
            Self::LimitExceeded(_) => "limit_exceeded",
            Self::UnsupportedByType { .. } => "unsupported_by_type",
            Self::CyclicComposition => "cyclic_composition",
            Self::Msg(_) => "exec_error",
        }
    }
}

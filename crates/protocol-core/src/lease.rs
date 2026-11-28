//! Per-key leadership-lease provider seam for ordered (stream) appends (`017` T038).
//!
//! A total-order stream append MUST present the current leadership epoch for its
//! key (research R6, constitution XII). The concrete lease registry lives outside
//! this crate: `crates/controlplane/src/lease.rs` owns the `006` primitives and
//! `crates/node` wires them behind this trait, while the Redis handler consumes
//! only the seam so it stays free of a control-plane dependency.

pub trait StreamLeaseProvider: Send + Sync {
    /// Current epoch for `key`, minting one if this node is the holder.
    fn grant(&self, key: &str) -> Result<u64, StreamLeaseError>;
    /// Fence one append: `presented` must equal the current epoch.
    fn check(&self, key: &str, presented: u64) -> Result<(), StreamLeaseError>;
    /// Record a persisted epoch on restore / test setup.
    fn adopt(&self, key: &str, epoch: u64);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamLeaseError {
    NotGranted,
    Stale { have: u64, need: u64 },
}

impl std::fmt::Display for StreamLeaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotGranted => write!(f, "leadership lease not held"),
            Self::Stale { have, need } => {
                write!(f, "stale leadership lease epoch (presented {have}, current {need})")
            }
        }
    }
}

impl std::error::Error for StreamLeaseError {}

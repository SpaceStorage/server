//! Internode fabric (012) — frames, auth, heartbeat, leaderless fanout wire helpers.

pub mod auth;
pub mod backpressure;
pub mod fanout;
pub mod frame;
pub mod heartbeat;
pub mod registry;
pub mod rpc;
pub mod rtt;

pub use auth::verify_join_secret;
pub use fanout::{count_durable_ok, local_memory_ack, local_write_ack};
pub use frame::{decode_frame, encode_frame, Frame, FrameError, CURRENT_VERSION};
pub use heartbeat::{FailureDetectorView, PeerStatus};
pub use rpc::{FanoutRead, FanoutWrite, RpcAck};

use parking_lot::RwLock;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FabricError {
    #[error("internode_required")]
    InternodeRequired,
    #[error("replication_required")]
    ReplicationRequired,
    #[error("quorum_domain_required")]
    QuorumDomainRequired,
    #[error("{0}")]
    Msg(String),
}

#[derive(Debug, Clone)]
pub struct FabricConfig {
    pub internode_bound: bool,
    pub replication_bound: bool,
    pub quorum_domain: String,
}

impl FabricConfig {
    pub fn require_always_on(
        internode: bool,
        replication: bool,
        domain: impl Into<String>,
    ) -> Result<Self, FabricError> {
        if !internode {
            return Err(FabricError::InternodeRequired);
        }
        if !replication {
            return Err(FabricError::ReplicationRequired);
        }
        let quorum_domain = domain.into();
        if quorum_domain.is_empty() {
            return Err(FabricError::QuorumDomainRequired);
        }
        Ok(Self {
            internode_bound: true,
            replication_bound: true,
            quorum_domain,
        })
    }
}

/// Shared runtime for internodes / replication accept loops.
#[derive(Clone)]
pub struct FabricRuntime {
    pub config: Arc<FabricConfig>,
    join_secret: Arc<RwLock<Vec<u8>>>,
    pub fd: Arc<FailureDetectorView>,
}

impl FabricRuntime {
    pub fn new(config: FabricConfig, join_secret: Vec<u8>) -> Self {
        Self {
            config: Arc::new(config),
            join_secret: Arc::new(RwLock::new(join_secret)),
            fd: Arc::new(FailureDetectorView::new(
                std::time::Duration::from_secs(15),
            )),
        }
    }

    /// Replace the join secret after membership bootstrap/restore (T073).
    pub fn set_join_secret(&self, secret: Vec<u8>) {
        *self.join_secret.write() = secret;
    }

    pub fn join_secret_bytes(&self) -> Vec<u8> {
        self.join_secret.read().clone()
    }

    pub fn verify_presented_secret(&self, presented: &[u8]) -> bool {
        let expected = self.join_secret.read();
        verify_join_secret(expected.as_slice(), presented)
    }
}

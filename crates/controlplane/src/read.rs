//! Follower metadata reads (read-index / applied ≥ commit).

use crate::error::{ControlPlaneError, Result};
use crate::group::GroupId;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ReadIndex {
    pub group: GroupId,
    pub applied_index: u64,
    pub commit_index: u64,
    pub leader: Option<Uuid>,
    pub is_leader: bool,
}

impl ReadIndex {
    /// Learners / lagging followers MUST wait or redirect rather than return stale-as-complete.
    pub fn allow_consistent_read(&self) -> Result<()> {
        if self.applied_index >= self.commit_index {
            return Ok(());
        }
        if self.is_leader {
            return Ok(());
        }
        Err(ControlPlaneError::NotLeader {
            group: self.group,
            leader: self.leader,
        })
    }
}

//! Boot restore orchestration: open Raft dirs → apply → invoke 013 content restore.

use crate::error::{ControlPlaneError, Result};
use crate::group::GroupId;
use crate::raft_store::RaftStore;
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct RestorePlan {
    pub groups: Vec<GroupId>,
    pub memory_content_empty: bool,
    pub not_member: bool,
}

impl RestorePlan {
    /// Ordered: cluster first, then each namespace group under `{data_dir}/raft/ns/`.
    pub async fn discover(data_dir: &Path) -> Result<Self> {
        let raft_root = data_dir.join("raft");
        let mut groups = vec![GroupId::Cluster];
        let ns_root = raft_root.join("ns");
        if ns_root.is_dir() {
            let ns_root_c = ns_root.clone();
            let ids = tokio::task::spawn_blocking(move || -> Result<Vec<Uuid>> {
                let mut out = Vec::new();
                let rd = std::fs::read_dir(&ns_root_c).map_err(|e| ControlPlaneError::Io(e.to_string()))?;
                for e in rd.flatten() {
                    if let Ok(name) = e.file_name().into_string() {
                        if let Ok(id) = Uuid::parse_str(&name) {
                            out.push(id);
                        }
                    }
                }
                Ok(out)
            })
            .await
            .map_err(|e| ControlPlaneError::Io(e.to_string()))??;
            for id in ids {
                groups.push(GroupId::Namespace(id));
            }
        }
        Ok(Self {
            groups,
            memory_content_empty: true,
            not_member: false,
        })
    }

    /// Open each group store; unknown format → quarantine + error.
    pub async fn open_stores(&self, data_dir: &Path) -> Result<Vec<RaftStore>> {
        let mut stores = Vec::new();
        for g in &self.groups {
            match RaftStore::open(data_dir, *g).await {
                Ok(s) => stores.push(s),
                Err(ControlPlaneError::UnknownRaftFormat { version }) => {
                    let _ = RaftStore::quarantine(data_dir, *g).await;
                    return Err(ControlPlaneError::UnknownRaftFormat { version });
                }
                Err(e) => return Err(e),
            }
        }
        Ok(stores)
    }
}

/// Non-member processes must not vote (FR-010).
pub fn may_vote(is_member: bool) -> Result<()> {
    if !is_member {
        return Err(ControlPlaneError::NotMember);
    }
    Ok(())
}

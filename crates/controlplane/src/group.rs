//! Raft group identifiers (`cluster` or `ns/<uuid>` on disk).

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GroupId {
    Cluster,
    Namespace(Uuid),
}

impl GroupId {
    /// On-disk directory name under `{data_dir}/raft/`.
    pub fn disk_id(self) -> String {
        match self {
            Self::Cluster => "cluster".into(),
            Self::Namespace(id) => format!("ns/{id}"),
        }
    }

    pub fn raft_dir(self, data_dir: &Path) -> PathBuf {
        data_dir.join("raft").join(self.disk_id())
    }

    pub fn is_cluster(self) -> bool {
        matches!(self, Self::Cluster)
    }
}

impl fmt::Display for GroupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cluster => write!(f, "cluster"),
            Self::Namespace(id) => write!(f, "ns/{id}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_ids() {
        assert_eq!(GroupId::Cluster.disk_id(), "cluster");
        let id = Uuid::nil();
        assert_eq!(
            GroupId::Namespace(id).disk_id(),
            format!("ns/{id}")
        );
    }
}

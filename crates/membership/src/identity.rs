//! Local identity directory: `{data_dir}/identity/{node,cluster}.json`.
//! Fsync via `spawn_blocking` (constitution II).

use crate::error::{MembershipError, Result};
use crate::secret::SecretEpoch;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeIdentityFile {
    pub node_id: Uuid,
    pub node_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClusterIdentityFile {
    pub cluster_uuid: Uuid,
    pub cluster_name: String,
    pub secret_epochs: Vec<SecretEpoch>,
    /// Quorum domain assigned at bootstrap / join (012).
    #[serde(default = "default_domain")]
    pub quorum_domain: String,
}

fn default_domain() -> String {
    "default".into()
}

pub fn identity_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("identity")
}

pub fn node_path(data_dir: &Path) -> PathBuf {
    identity_dir(data_dir).join("node.json")
}

pub fn cluster_path(data_dir: &Path) -> PathBuf {
    identity_dir(data_dir).join("cluster.json")
}

async fn write_json_fsync(path: PathBuf, bytes: Vec<u8>) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| MembershipError::Io(e.to_string()))?;
        }
        std::fs::write(&path, &bytes).map_err(|e| MembershipError::Io(e.to_string()))?;
        let f = std::fs::File::open(&path).map_err(|e| MembershipError::Io(e.to_string()))?;
        f.sync_all().map_err(|e| MembershipError::Io(e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = f.metadata().map_err(|e| MembershipError::Io(e.to_string()))?.permissions();
            perms.set_mode(0o600);
            std::fs::set_permissions(&path, perms).map_err(|e| MembershipError::Io(e.to_string()))?;
        }
        Ok(())
    })
    .await
    .map_err(|e| MembershipError::Io(e.to_string()))?
}

pub async fn load_node(data_dir: &Path) -> Result<Option<NodeIdentityFile>> {
    let path = node_path(data_dir);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| MembershipError::Io(e.to_string()))?;
    let file: NodeIdentityFile =
        serde_json::from_slice(&bytes).map_err(|e| MembershipError::Io(e.to_string()))?;
    Ok(Some(file))
}

pub async fn load_cluster(data_dir: &Path) -> Result<Option<ClusterIdentityFile>> {
    let path = cluster_path(data_dir);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| MembershipError::Io(e.to_string()))?;
    let file: ClusterIdentityFile =
        serde_json::from_slice(&bytes).map_err(|e| MembershipError::Io(e.to_string()))?;
    Ok(Some(file))
}

pub async fn load_or_create_node(data_dir: &Path, node_name: &str) -> Result<NodeIdentityFile> {
    if let Some(existing) = load_node(data_dir).await? {
        return Ok(existing);
    }
    let file = NodeIdentityFile {
        node_id: Uuid::new_v4(),
        node_name: node_name.to_string(),
    };
    save_node(data_dir, &file).await?;
    Ok(file)
}

pub async fn save_node(data_dir: &Path, file: &NodeIdentityFile) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(file).map_err(|e| MembershipError::Io(e.to_string()))?;
    write_json_fsync(node_path(data_dir), bytes).await
}

pub async fn save_cluster(data_dir: &Path, file: &ClusterIdentityFile) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(file).map_err(|e| MembershipError::Io(e.to_string()))?;
    write_json_fsync(cluster_path(data_dir), bytes).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn load_or_create_stable_id() {
        let dir = tempdir().unwrap();
        let a = load_or_create_node(dir.path(), "n1").await.unwrap();
        let b = load_or_create_node(dir.path(), "n1").await.unwrap();
        assert_eq!(a.node_id, b.node_id);
        assert_eq!(a.node_name, "n1");
    }
}

//! Explicit bootstrap: cluster UUID + secret epoch 1 + sole ready member.

use crate::error::{MembershipError, Result};
use crate::events::{MemberStatus, MembershipEvent};
use crate::identity::{self, ClusterIdentityFile, NodeIdentityFile};
use crate::secret::{self, SecretEpoch};
use crate::view::{MemberRecord, MembershipView};
use std::path::Path;
use uuid::Uuid;

pub struct BootstrapParams<'a> {
    pub data_dir: &'a Path,
    pub node_name: &'a str,
    pub cluster_name: &'a str,
    pub quorum_domain: &'a str,
    pub token_file: Option<&'a Path>,
    /// Non-self seeds → refuse.
    pub foreign_seeds: bool,
    pub labels: std::collections::BTreeMap<String, String>,
}

#[derive(Debug)]
pub struct BootstrapOutcome {
    pub view: MembershipView,
    pub node: NodeIdentityFile,
    pub cluster: ClusterIdentityFile,
    pub event: MembershipEvent,
    pub join_secret_hex: String,
}

/// Require `cluster { bootstrap; }`, no foreign seeds, and no local `cluster.json`.
pub async fn bootstrap(p: BootstrapParams<'_>) -> Result<BootstrapOutcome> {
    if p.foreign_seeds {
        return Err(MembershipError::BootstrapForeignSeeds);
    }
    if identity::load_cluster(p.data_dir).await?.is_some() {
        return Err(MembershipError::InvalidState(
            "cluster.json already exists; ignore bootstrap".into(),
        ));
    }

    let node = identity::load_or_create_node(p.data_dir, p.node_name).await?;
    let epoch = secret::generate_bootstrap_secret();
    let cluster_uuid = Uuid::new_v4();
    let cluster = ClusterIdentityFile {
        cluster_uuid,
        cluster_name: p.cluster_name.to_string(),
        secret_epochs: vec![epoch.clone()],
        quorum_domain: p.quorum_domain.to_string(),
    };
    identity::save_cluster(p.data_dir, &cluster).await?;

    if let Some(tf) = p.token_file {
        secret::write_token_file(tf, &epoch.secret_hex).await?;
    }

    let member = MemberRecord {
        node_id: node.node_id,
        node_name: node.node_name.clone(),
        status: MemberStatus::Ready,
        incarnation: 1,
        quorum_domain: p.quorum_domain.to_string(),
        voter: true,
        labels: p.labels.clone(),
    };
    let mut view = MembershipView::empty(cluster_uuid, p.cluster_name, 1);
    view.members.insert(node.node_id, member);
    view.quorum_domain_default = p.quorum_domain.to_string();
    view.ensure_domain(p.quorum_domain);

    let event = MembershipEvent::Bootstrap {
        cluster_uuid,
        cluster_name: p.cluster_name.to_string(),
        node_id: node.node_id,
        node_name: node.node_name.clone(),
        quorum_domain: p.quorum_domain.to_string(),
    };

    Ok(BootstrapOutcome {
        view,
        node,
        cluster,
        event,
        join_secret_hex: epoch.secret_hex,
    })
}

/// Restart path: load existing cluster identity into a one-member (or restored) view seed.
pub async fn load_existing(
    data_dir: &Path,
    node_name: &str,
) -> Result<Option<(NodeIdentityFile, ClusterIdentityFile, Vec<SecretEpoch>)>> {
    let Some(cluster) = identity::load_cluster(data_dir).await? else {
        return Ok(None);
    };
    let node = identity::load_or_create_node(data_dir, node_name).await?;
    let epochs = cluster.secret_epochs.clone();
    Ok(Some((node, cluster, epochs)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn bootstrap_creates_uuid_and_secret() {
        let dir = tempdir().unwrap();
        let token = dir.path().join("join.token");
        let out = bootstrap(BootstrapParams {
            data_dir: dir.path(),
            node_name: "n1",
            cluster_name: "lab",
            quorum_domain: "lab",
            token_file: Some(&token),
            foreign_seeds: false,
            labels: Default::default(),
        })
        .await
        .unwrap();
        assert_eq!(out.view.members.len(), 1);
        assert!(token.exists());
        let again = bootstrap(BootstrapParams {
            data_dir: dir.path(),
            node_name: "n1",
            cluster_name: "lab",
            quorum_domain: "lab",
            token_file: Some(&token),
            foreign_seeds: false,
            labels: Default::default(),
        })
        .await;
        assert!(matches!(again, Err(MembershipError::InvalidState(_))));
    }

    #[tokio::test]
    async fn foreign_seeds_refused() {
        let dir = tempdir().unwrap();
        let err = bootstrap(BootstrapParams {
            data_dir: dir.path(),
            node_name: "n1",
            cluster_name: "lab",
            quorum_domain: "lab",
            token_file: None,
            foreign_seeds: true,
            labels: Default::default(),
        })
        .await
        .unwrap_err();
        assert_eq!(err, MembershipError::BootstrapForeignSeeds);
    }

    #[tokio::test]
    async fn two_isolated_bootstraps_different_uuid() {
        let a = tempdir().unwrap();
        let b = tempdir().unwrap();
        let oa = bootstrap(BootstrapParams {
            data_dir: a.path(),
            node_name: "n1",
            cluster_name: "same",
            quorum_domain: "default",
            token_file: None,
            foreign_seeds: false,
            labels: Default::default(),
        })
        .await
        .unwrap();
        let ob = bootstrap(BootstrapParams {
            data_dir: b.path(),
            node_name: "n1",
            cluster_name: "same",
            quorum_domain: "default",
            token_file: None,
            foreign_seeds: false,
            labels: Default::default(),
        })
        .await
        .unwrap();
        assert_ne!(oa.cluster.cluster_uuid, ob.cluster.cluster_uuid);
    }
}

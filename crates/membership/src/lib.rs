//! Cluster identity / membership procedures (011).
//!
//! Replaces the secret-only `spacestorage-identity` stub: join requires
//! secret + admit **or** one-time token; pending joins are not voters or
//! replica targets.

pub mod bootstrap;
pub mod decommission;
pub mod drain;
pub mod error;
pub mod events;
pub mod identity;
pub mod join;
pub mod metrics;
pub mod replace;
pub mod secret;
pub mod seed_client;
pub mod token;
pub mod view;

pub use bootstrap::{bootstrap, BootstrapOutcome, BootstrapParams};
pub use error::{MembershipError, Result};
pub use events::{MemberStatus, MembershipEvent};
pub use join::{
    ack_with_epochs, admit_pending, handle_join, mint_token, JoinAck, JoinRequest, SeedEndpoint,
};
pub use metrics::MembershipMetrics;
pub use view::{MemberRecord, MembershipView, PendingJoin};

use parking_lot::RwLock;
use secret::SecretEpoch;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

/// Startup / runtime membership handle. Called from `node` before `ready`.
pub struct MembershipService {
    data_dir: PathBuf,
    node_name: String,
    cluster_name: String,
    quorum_domain: String,
    token_file: Option<PathBuf>,
    bootstrap_declared: bool,
    join_declared: bool,
    foreign_seeds: bool,
    ladder: Vec<String>,
    labels: std::collections::BTreeMap<String, String>,
    inner: RwLock<ServiceState>,
    metrics: Arc<MembershipMetrics>,
}

struct ServiceState {
    view: Option<MembershipView>,
    node_id: Option<Uuid>,
    epochs: Vec<SecretEpoch>,
    /// True when this process is an admitted member (or sole bootstrap).
    admitted: bool,
    /// Pending-only (first join waiting for admit).
    pending_only: bool,
    events: Vec<MembershipEvent>,
}

pub struct MembershipStartOpts {
    pub data_dir: PathBuf,
    pub node_name: String,
    pub cluster_name: String,
    /// From `cluster.quorum_domain`, else `"default"`.
    pub quorum_domain: String,
    pub token_file: Option<PathBuf>,
    pub bootstrap: bool,
    pub join: bool,
    pub foreign_seeds: bool,
    pub topology_ladder: Vec<String>,
    /// Node topology labels (`node { labels { az … } }`) pinned on bootstrap member.
    pub labels: std::collections::BTreeMap<String, String>,
}

impl MembershipService {
    pub fn new(opts: MembershipStartOpts) -> Self {
        Self {
            data_dir: opts.data_dir,
            node_name: opts.node_name,
            cluster_name: opts.cluster_name,
            quorum_domain: opts.quorum_domain,
            token_file: opts.token_file,
            bootstrap_declared: opts.bootstrap,
            join_declared: opts.join,
            foreign_seeds: opts.foreign_seeds,
            ladder: opts.topology_ladder,
            labels: opts.labels,
            inner: RwLock::new(ServiceState {
                view: None,
                node_id: None,
                epochs: Vec::new(),
                admitted: false,
                pending_only: false,
                events: Vec::new(),
            }),
            metrics: Arc::new(MembershipMetrics::default()),
        }
    }

    pub fn metrics(&self) -> Arc<MembershipMetrics> {
        Arc::clone(&self.metrics)
    }

    pub fn is_admitted(&self) -> bool {
        self.inner.read().admitted
    }

    pub fn is_pending_only(&self) -> bool {
        self.inner.read().pending_only
    }

    /// Gate for `ready`: admitted member (bootstrap or prior join) only.
    pub fn may_become_ready(&self) -> bool {
        let g = self.inner.read();
        g.admitted && !g.pending_only
    }

    pub fn view_snapshot(&self) -> Option<MembershipView> {
        self.inner.read().view.clone()
    }

    pub fn node_id(&self) -> Option<Uuid> {
        self.inner.read().node_id
    }

    pub fn drain_events(&self) -> Vec<MembershipEvent> {
        std::mem::take(&mut self.inner.write().events)
    }

    /// Load or bootstrap identity. Must run before advertising `ready`.
    pub async fn on_start(&self) -> Result<()> {
        if self.bootstrap_declared && self.join_declared {
            return Err(MembershipError::BootstrapAndJoin);
        }

        // Existing cluster identity → restart as member (ignore leftover bootstrap).
        if let Some((node, cluster, epochs)) =
            bootstrap::load_existing(&self.data_dir, &self.node_name).await?
        {
            let mut view =
                MembershipView::empty(cluster.cluster_uuid, &cluster.cluster_name, epochs
                    .iter()
                    .map(|e| e.epoch)
                    .max()
                    .unwrap_or(1));
            view.quorum_domain_default = cluster.quorum_domain.clone();
            view.ensure_domain(&cluster.quorum_domain);
            view.members.insert(
                node.node_id,
                MemberRecord {
                    node_id: node.node_id,
                    node_name: node.node_name.clone(),
                    status: MemberStatus::Ready,
                    incarnation: 1,
                    quorum_domain: cluster.quorum_domain.clone(),
                    voter: true,
                    labels: Default::default(),
                },
            );
            let mut g = self.inner.write();
            g.node_id = Some(node.node_id);
            g.epochs = epochs;
            g.view = Some(view);
            g.admitted = true;
            g.pending_only = false;
            self.metrics.set_members(1);
            self.metrics.set_secret_epoch(
                g.epochs.iter().map(|e| e.epoch).max().unwrap_or(1),
            );
            info!(
                cluster_uuid = %cluster.cluster_uuid,
                node_id = %node.node_id,
                "membership: restored local cluster identity"
            );
            return Ok(());
        }

        if self.bootstrap_declared {
            let out = bootstrap::bootstrap(BootstrapParams {
                data_dir: &self.data_dir,
                node_name: &self.node_name,
                cluster_name: &self.cluster_name,
                quorum_domain: &self.quorum_domain,
                token_file: self.token_file.as_deref(),
                foreign_seeds: self.foreign_seeds,
                labels: self.labels.clone(),
            })
            .await?;
            let mut g = self.inner.write();
            g.node_id = Some(out.node.node_id);
            g.epochs = out.cluster.secret_epochs.clone();
            g.view = Some(out.view);
            g.admitted = true;
            g.pending_only = false;
            g.events.push(out.event);
            self.metrics.set_members(1);
            self.metrics.set_secret_epoch(1);
            info!(
                cluster_uuid = %out.cluster.cluster_uuid,
                node_id = %out.node.node_id,
                quorum_domain = %self.quorum_domain,
                "membership: bootstrap complete"
            );
            return Ok(());
        }

        if self.join_declared {
            // First-binary join: create local node id; remain not-ready until admit/token
            // completes via internodes (caller drives handle_join). Secret must exist.
            if self.token_file.is_none() {
                return Err(MembershipError::JoinSecretRequired);
            }
            let node = identity::load_or_create_node(&self.data_dir, &self.node_name).await?;
            let mut g = self.inner.write();
            g.node_id = Some(node.node_id);
            g.admitted = false;
            g.pending_only = true;
            warn!(
                node_id = %node.node_id,
                "membership: join mode — not ready until admit or token"
            );
            return Ok(());
        }

        Err(MembershipError::InvalidState(
            "bootstrap_or_join_required".into(),
        ))
    }

    /// Apply a remote/local join against the in-memory view (seed / leader path).
    pub fn apply_join(&self, req: JoinRequest) -> Result<JoinAck> {
        let mut g = self.inner.write();
        let epochs = g.epochs.clone();
        let Some(view) = g.view.as_mut() else {
            return Err(MembershipError::InvalidState("no_view".into()));
        };
        let domains = view.quorum_domains.clone();
        let (ack, events) =
            handle_join(view, &epochs, req, &self.ladder, |d| domains.contains(d))?;
        let ack = ack_with_epochs(ack, &epochs);
        let members = view.members.len() as u64;
        let pending = view.pending.len() as u64;
        g.events.extend(events);
        match &ack {
            JoinAck::Pending => {
                self.metrics.set_pending(pending);
                self.metrics.join_ok();
            }
            JoinAck::Admitted { .. } | JoinAck::Replaced { .. } => {
                self.metrics.set_members(members);
                self.metrics.set_pending(pending);
                self.metrics.join_ok();
            }
            JoinAck::Refused { .. } => self.metrics.join_refused(),
        }
        Ok(ack)
    }

    pub fn apply_admit(&self, node_id: Uuid) -> Result<JoinAck> {
        let mut g = self.inner.write();
        let epochs = g.epochs.clone();
        let Some(view) = g.view.as_mut() else {
            return Err(MembershipError::InvalidState("no_view".into()));
        };
        let (ack, events) = admit_pending(view, node_id)?;
        let ack = ack_with_epochs(ack, &epochs);
        let members = view.members.len() as u64;
        let pending = view.pending.len() as u64;
        g.events.extend(events);
        self.metrics.set_members(members);
        self.metrics.set_pending(pending);
        Ok(ack)
    }

    /// Mark this process admitted after successful JoinAck::Admitted (joiner side).
    ///
    /// `members` is the seed roster from the ack so joiners converge on an identical view (G2).
    pub async fn mark_admitted_local(
        &self,
        cluster_uuid: Uuid,
        cluster_name: &str,
        epochs: Vec<SecretEpoch>,
        quorum_domain: &str,
        members: Vec<crate::view::MemberRecord>,
    ) -> Result<()> {
        let node = identity::load_or_create_node(&self.data_dir, &self.node_name).await?;
        let cluster = identity::ClusterIdentityFile {
            cluster_uuid,
            cluster_name: cluster_name.to_string(),
            secret_epochs: epochs.clone(),
            quorum_domain: quorum_domain.to_string(),
        };
        identity::save_cluster(&self.data_dir, &cluster).await?;
        let mut view = MembershipView::empty(cluster_uuid, cluster_name, epochs
            .iter()
            .map(|e| e.epoch)
            .max()
            .unwrap_or(1));
        view.quorum_domain_default = quorum_domain.to_string();
        view.ensure_domain(quorum_domain);
        if members.is_empty() {
            view.members.insert(
                node.node_id,
                MemberRecord {
                    node_id: node.node_id,
                    node_name: node.node_name.clone(),
                    status: MemberStatus::Ready,
                    incarnation: 1,
                    quorum_domain: quorum_domain.to_string(),
                    voter: true,
                    labels: self.labels.clone(),
                },
            );
        } else {
            for m in members {
                view.members.insert(m.node_id, m);
            }
        }
        view.recompute_voters();
        let n = view.members.len() as u64;
        let mut g = self.inner.write();
        g.node_id = Some(node.node_id);
        g.epochs = epochs;
        g.view = Some(view);
        g.admitted = true;
        g.pending_only = false;
        self.metrics.set_members(n);
        Ok(())
    }

    /// Mint a one-time join token on the bootstrap/seed view (harness / admin seam).
    pub fn mint_join_token(
        &self,
        node_name: &str,
        node_id: Option<Uuid>,
        ttl: std::time::Duration,
    ) -> Result<crate::token::JoinToken> {
        let mut g = self.inner.write();
        let Some(view) = g.view.as_mut() else {
            return Err(MembershipError::InvalidState("no_view".into()));
        };
        let (token, event) = mint_token(view, node_name, node_id, ttl)?;
        g.events.push(event);
        Ok(token)
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// First accepted join-secret bytes for fabric injection (T073).
    pub fn accepted_join_secret(&self) -> Option<Vec<u8>> {
        let g = self.inner.read();
        g.epochs
            .iter()
            .find(|e| e.accepted)
            .and_then(|e| e.secret_bytes().ok())
    }

    /// Load join secret from `token_file` (joiner path before epochs exist).
    pub async fn load_presented_secret(&self) -> Result<Vec<u8>> {
        let Some(path) = self.token_file.as_ref() else {
            return Err(MembershipError::JoinSecretRequired);
        };
        secret::read_token_file(path).await
    }

    /// Drive first-join: contact seeds with `JoinRequest`, honor `JoinAck`.
    /// On `Admitted` / `Replaced`, calls [`Self::mark_admitted_local`].
    /// On `Pending`, leaves `pending_only` so the process can stay up (T053/T054).
    pub async fn drive_first_join(
        &self,
        seeds: &[SeedEndpoint],
        labels: std::collections::BTreeMap<String, String>,
        internodes_address: String,
        join_token: Option<Uuid>,
    ) -> Result<JoinAck> {
        if !self.join_declared || !self.is_pending_only() {
            return Err(MembershipError::InvalidState("not_join_pending".into()));
        }
        let node_id = self
            .node_id()
            .ok_or_else(|| MembershipError::InvalidState("no_node_id".into()))?;
        let secret = self.load_presented_secret().await?;
        let req = JoinRequest {
            node_id,
            node_name: self.node_name.clone(),
            labels,
            internodes_address,
            quorum_domain: self.quorum_domain.clone(),
            presented_secret: secret.clone(),
            join_token,
            replace_of: None,
        };
        let ack = seed_client::contact_seeds(seeds, &secret, &req).await?;
        match &ack {
            JoinAck::Admitted {
                cluster_uuid,
                cluster_name,
                quorum_domain,
                members,
                epochs,
                ..
            }
            | JoinAck::Replaced {
                cluster_uuid,
                cluster_name,
                quorum_domain,
                members,
                epochs,
                ..
            } => {
                self.mark_admitted_local(
                    *cluster_uuid,
                    cluster_name,
                    epochs.clone(),
                    quorum_domain,
                    members.clone(),
                )
                .await?;
                info!(
                    cluster_uuid = %cluster_uuid,
                    "membership: first-join admitted locally"
                );
            }
            JoinAck::Pending => {
                info!("membership: seed ack pending — waiting for admit");
            }
            JoinAck::Refused { code } => {
                warn!(code = %code, "membership: join refused by seed");
            }
        }
        Ok(ack)
    }

    /// Apply a membership roster from the control-plane cluster store (006 Raft).
    ///
    /// Prefer this over [`Self::refresh_roster_from_seeds`] once RaftClusterStore
    /// is the source of truth; seed pull remains a join-bootstrap fallback (FR-009).
    pub fn apply_controlplane_roster(&self, members: Vec<crate::view::MemberRecord>) -> Result<()> {
        self.apply_roster(members)
    }

    /// Pull the current seed roster into the local view (FR-009 without Raft).
    ///
    /// Already-admitted members re-present as restart joins; the seed returns
    /// [`JoinAck::Admitted`] with the full member list so earlier joiners learn
    /// about later admits. When control-plane Raft is live, prefer
    /// [`Self::apply_controlplane_roster`].
    pub async fn refresh_roster_from_seeds(&self, seeds: &[SeedEndpoint]) -> Result<JoinAck> {
        if !self.is_admitted() {
            return Err(MembershipError::InvalidState("not_admitted".into()));
        }
        let node_id = self
            .node_id()
            .ok_or_else(|| MembershipError::InvalidState("no_node_id".into()))?;
        let secret = self
            .accepted_join_secret()
            .ok_or(MembershipError::JoinSecretRequired)?;
        let req = JoinRequest {
            node_id,
            node_name: self.node_name.clone(),
            labels: self.labels.clone(),
            internodes_address: String::new(),
            quorum_domain: self.quorum_domain.clone(),
            presented_secret: secret.clone(),
            join_token: None,
            replace_of: None,
        };
        let ack = seed_client::contact_seeds(seeds, &secret, &req).await?;
        match &ack {
            JoinAck::Admitted { members, .. } | JoinAck::Replaced { members, .. } => {
                self.apply_roster(members.clone())?;
                info!(members = members.len(), "membership: roster refreshed from seed");
            }
            JoinAck::Pending => {
                return Err(MembershipError::InvalidState(
                    "refresh_got_pending".into(),
                ));
            }
            JoinAck::Refused { code } => {
                return Err(MembershipError::InvalidState(format!(
                    "refresh_refused:{code}"
                )));
            }
        }
        Ok(ack)
    }

    /// Replace the local member table with a seed-provided roster (keeps local domains).
    pub fn apply_roster(&self, members: Vec<MemberRecord>) -> Result<()> {
        let mut g = self.inner.write();
        let Some(view) = g.view.as_mut() else {
            return Err(MembershipError::InvalidState("no_view".into()));
        };
        view.members.clear();
        for m in members {
            view.members.insert(m.node_id, m);
        }
        view.recompute_voters();
        let n = view.members.len() as u64;
        self.metrics.set_members(n);
        Ok(())
    }

    /// True when this process is in first-join mode (may wait without ready).
    pub fn is_join_mode(&self) -> bool {
        self.join_declared
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn on_start_bootstrap_gates_ready() {
        let dir = tempdir().unwrap();
        let svc = MembershipService::new(MembershipStartOpts {
            data_dir: dir.path().to_path_buf(),
            node_name: "n1".into(),
            cluster_name: "lab".into(),
            quorum_domain: "lab".into(),
            token_file: Some(dir.path().join("join.token")),
            bootstrap: true,
            join: false,
            foreign_seeds: false,
            topology_ladder: vec!["az".into()],
            labels: std::collections::BTreeMap::from([("az".into(), "a".into())]),
        });
        svc.on_start().await.unwrap();
        assert!(svc.may_become_ready());
        assert!(svc.is_admitted());
        assert!(dir.path().join("identity/cluster.json").exists());
        assert!(dir.path().join("join.token").exists());
        let v = svc.view_snapshot().unwrap();
        assert_eq!(v.members.values().next().unwrap().labels.get("az").map(String::as_str), Some("a"));
    }

    #[tokio::test]
    async fn join_without_admit_not_ready() {
        let dir = tempdir().unwrap();
        // Need a secret file present for join mode.
        std::fs::write(dir.path().join("join.token"), "00").unwrap();
        let svc = MembershipService::new(MembershipStartOpts {
            data_dir: dir.path().to_path_buf(),
            node_name: "n2".into(),
            cluster_name: "lab".into(),
            quorum_domain: "lab".into(),
            token_file: Some(dir.path().join("join.token")),
            bootstrap: false,
            join: true,
            foreign_seeds: false,
            topology_ladder: vec!["az".into()],
            labels: Default::default(),
        });
        svc.on_start().await.unwrap();
        assert!(!svc.may_become_ready());
        assert!(svc.is_pending_only());
        assert!(svc.is_join_mode());
    }

    #[tokio::test]
    async fn mark_admitted_local_allows_ready() {
        let dir = tempdir().unwrap();
        let secret = crate::secret::generate_bootstrap_secret();
        std::fs::write(dir.path().join("join.token"), &secret.secret_hex).unwrap();
        let svc = MembershipService::new(MembershipStartOpts {
            data_dir: dir.path().to_path_buf(),
            node_name: "n2".into(),
            cluster_name: "lab".into(),
            quorum_domain: "lab".into(),
            token_file: Some(dir.path().join("join.token")),
            bootstrap: false,
            join: true,
            foreign_seeds: false,
            topology_ladder: vec!["az".into()],
            labels: Default::default(),
        });
        svc.on_start().await.unwrap();
        assert!(!svc.may_become_ready());
        let cu = Uuid::new_v4();
        svc.mark_admitted_local(cu, "lab", vec![secret], "lab", vec![])
            .await
            .unwrap();
        assert!(svc.may_become_ready());
        assert!(!svc.is_pending_only());
    }
}

//! First-join handshake: secret + ladder → pending, or token → admit.

use crate::error::{MembershipError, Result};
use crate::events::{MemberStatus, MembershipEvent};
use crate::secret::{self, SecretEpoch};
use crate::token::JoinToken;
use crate::view::{MemberRecord, MembershipView, PendingJoin};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinRequest {
    pub node_id: Uuid,
    pub node_name: String,
    pub labels: BTreeMap<String, String>,
    pub internodes_address: String,
    pub quorum_domain: String,
    pub presented_secret: Vec<u8>,
    pub join_token: Option<Uuid>,
    /// Replace path: present existing member id.
    pub replace_of: Option<Uuid>,
    /// Product major version for N/N+1 rolling upgrade window (015). Default 1.
    #[serde(default = "default_product_version")]
    pub product_version: u16,
}

fn default_product_version() -> u16 {
    1
}

/// Wire + local ack for first-join (`pending` \| `admitted` \| `replaced` \| `refused`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum JoinAck {
    Pending,
    Admitted {
        incarnation: u64,
        cluster_uuid: Uuid,
        cluster_name: String,
        quorum_domain: String,
        /// Full member roster so joiners share an identical view (G2).
        #[serde(default)]
        members: Vec<MemberRecord>,
        epochs: Vec<SecretEpoch>,
    },
    Replaced {
        incarnation: u64,
        cluster_uuid: Uuid,
        cluster_name: String,
        quorum_domain: String,
        #[serde(default)]
        members: Vec<MemberRecord>,
        epochs: Vec<SecretEpoch>,
    },
    Refused { code: String },
}

fn admitted_ack(
    view: &MembershipView,
    epochs: &[SecretEpoch],
    incarnation: u64,
) -> JoinAck {
    JoinAck::Admitted {
        incarnation,
        cluster_uuid: view.cluster_uuid,
        cluster_name: view.cluster_name.clone(),
        quorum_domain: view.quorum_domain_default.clone(),
        members: view.members.values().cloned().collect(),
        epochs: epochs.to_vec(),
    }
}

/// Seed discovery endpoint for first-join (`cluster { seeds { … } }`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeedEndpoint {
    pub name: String,
    pub address: String,
    pub port: u16,
}

impl SeedEndpoint {
    pub fn host_port(&self) -> String {
        format!("{}:{}", self.address, self.port)
    }
}

/// Required ladder keys must all be present in labels (004).
pub fn ladder_ok(labels: &BTreeMap<String, String>, ladder: &[String]) -> bool {
    ladder.iter().all(|k| labels.contains_key(k))
}

pub fn handle_join(
    view: &mut MembershipView,
    epochs: &[crate::secret::SecretEpoch],
    req: JoinRequest,
    ladder: &[String],
    domain_exists: impl Fn(&str) -> bool,
) -> Result<(JoinAck, Vec<MembershipEvent>)> {
    handle_join_with_version(
        view,
        epochs,
        req,
        ladder,
        domain_exists,
        spacestorage_compat::ProductVersion::FIRST_BINARY,
    )
}

/// Join with an explicit local product version (N/N+1 window, 015).
pub fn handle_join_with_version(
    view: &mut MembershipView,
    epochs: &[crate::secret::SecretEpoch],
    req: JoinRequest,
    ladder: &[String],
    domain_exists: impl Fn(&str) -> bool,
    local_version: spacestorage_compat::ProductVersion,
) -> Result<(JoinAck, Vec<MembershipEvent>)> {
    let mut events = Vec::new();

    let peer = spacestorage_compat::ProductVersion(req.product_version);
    if spacestorage_compat::ProductVersion::check_peer(local_version, peer).is_err() {
        return Ok((
            JoinAck::Refused {
                code: "product_version_window".into(),
            },
            events,
        ));
    }

    if !secret::verify_secret(epochs, &req.presented_secret) {
        return Ok((
            JoinAck::Refused {
                code: "secret_mismatch".into(),
            },
            events,
        ));
    }
    if view.retired.contains(&req.node_id) {
        return Ok((
            JoinAck::Refused {
                code: "retired_identity".into(),
            },
            events,
        ));
    }
    if !ladder_ok(&req.labels, ladder) {
        return Ok((
            JoinAck::Refused {
                code: "ladder".into(),
            },
            events,
        ));
    }
    if req.quorum_domain.is_empty() {
        return Ok((
            JoinAck::Refused {
                code: "quorum_domain_required".into(),
            },
            events,
        ));
    }
    if !domain_exists(&req.quorum_domain) {
        return Ok((
            JoinAck::Refused {
                code: "quorum_domain_unknown".into(),
            },
            events,
        ));
    }
    if view
        .members
        .values()
        .any(|m| m.node_name == req.node_name && m.node_id != req.node_id)
    {
        return Ok((
            JoinAck::Refused {
                code: "name_in_use".into(),
            },
            events,
        ));
    }

    // Already a member → ack admitted (restart).
    if let Some(m) = view.members.get(&req.node_id) {
        return Ok((admitted_ack(view, epochs, m.incarnation), events));
    }

    // Token path → AdmitMember without pending.
    if let Some(tid) = req.join_token {
        let Some(token) = view.tokens.get_mut(&tid) else {
            return Ok((
                JoinAck::Refused {
                    code: "token_refused".into(),
                },
                events,
            ));
        };
        if let Err(e) = token.consume(&req.node_name, req.node_id) {
            return Ok((
                JoinAck::Refused {
                    code: e.code().into(),
                },
                events,
            ));
        }
        events.push(MembershipEvent::JoinTokenConsume {
            token_id: tid,
            node_id: req.node_id,
        });
        let ack = admit_member(view, &req, &mut events)?;
        return Ok((ack, events));
    }

    // Pending path.
    let pending = PendingJoin {
        node_id: req.node_id,
        node_name: req.node_name.clone(),
        labels: req.labels.clone(),
        internodes_address: req.internodes_address.clone(),
        quorum_domain: req.quorum_domain.clone(),
        token_id: None,
        connected: true,
    };
    view.pending.insert(req.node_id, pending);
    events.push(MembershipEvent::PendingJoin {
        node_id: req.node_id,
        node_name: req.node_name,
        labels: req.labels.into_iter().collect(),
        internodes_address: req.internodes_address,
        token_id: None,
    });
    Ok((JoinAck::Pending, events))
}

fn admit_member(
    view: &mut MembershipView,
    req: &JoinRequest,
    events: &mut Vec<MembershipEvent>,
) -> Result<JoinAck> {
    let member = MemberRecord {
        node_id: req.node_id,
        node_name: req.node_name.clone(),
        status: MemberStatus::Ready,
        incarnation: 1,
        quorum_domain: req.quorum_domain.clone(),
        voter: false,
        labels: req.labels.clone(),
    };
    view.pending.remove(&req.node_id);
    view.members.insert(req.node_id, member);
    view.recompute_voters();
    events.push(MembershipEvent::AdmitMember {
        node_id: req.node_id,
        node_name: req.node_name.clone(),
        quorum_domain: req.quorum_domain.clone(),
        incarnation: 1,
    });
    // Epochs filled by caller (`apply_join` / `apply_admit`) via `with_epochs`.
    Ok(JoinAck::Admitted {
        incarnation: 1,
        cluster_uuid: view.cluster_uuid,
        cluster_name: view.cluster_name.clone(),
        quorum_domain: view.quorum_domain_default.clone(),
        members: view.members.values().cloned().collect(),
        epochs: Vec::new(),
    })
}

/// Attach secret epochs to an admit/replace ack (seed → joiner).
pub fn ack_with_epochs(ack: JoinAck, epochs: &[SecretEpoch]) -> JoinAck {
    match ack {
        JoinAck::Admitted {
            incarnation,
            cluster_uuid,
            cluster_name,
            quorum_domain,
            members,
            ..
        } => JoinAck::Admitted {
            incarnation,
            cluster_uuid,
            cluster_name,
            quorum_domain,
            members,
            epochs: epochs.to_vec(),
        },
        JoinAck::Replaced {
            incarnation,
            cluster_uuid,
            cluster_name,
            quorum_domain,
            members,
            ..
        } => JoinAck::Replaced {
            incarnation,
            cluster_uuid,
            cluster_name,
            quorum_domain,
            members,
            epochs: epochs.to_vec(),
        },
        other => other,
    }
}

/// Operator admit of a connected pending join.
pub fn admit_pending(
    view: &mut MembershipView,
    node_id: Uuid,
) -> Result<(JoinAck, Vec<MembershipEvent>)> {
    let Some(pending) = view.pending.get(&node_id).cloned() else {
        return Err(MembershipError::NotPending);
    };
    if !pending.connected {
        return Ok((
            JoinAck::Refused {
                code: "pending_disconnected".into(),
            },
            vec![],
        ));
    }
    let req = JoinRequest {
        node_id: pending.node_id,
        node_name: pending.node_name,
        labels: pending.labels,
        internodes_address: pending.internodes_address,
        quorum_domain: pending.quorum_domain,
        presented_secret: vec![],
        join_token: None,
        replace_of: None,
            product_version: 1,
    };
    let mut events = Vec::new();
    // Skip secret (already verified at pending).
    let ack = admit_member(view, &req, &mut events)?;
    Ok((ack, events))
}

pub fn drop_pending_on_disconnect(view: &mut MembershipView, node_id: Uuid) {
    if let Some(p) = view.pending.get_mut(&node_id) {
        p.connected = false;
    }
}

pub fn mint_token(
    view: &mut MembershipView,
    node_name: impl Into<String>,
    node_id: Option<Uuid>,
    ttl: std::time::Duration,
) -> Result<(JoinToken, MembershipEvent)> {
    let token = JoinToken::mint(node_name, node_id, ttl)?;
    let event = MembershipEvent::JoinTokenMint {
        token_id: token.token_id,
        node_name: token.node_name.clone(),
        node_id: token.node_id,
    };
    view.tokens.insert(token.token_id, token.clone());
    Ok((token, event))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret::generate_bootstrap_secret;
    use uuid::Uuid;

    fn base_view() -> (MembershipView, Vec<crate::secret::SecretEpoch>) {
        let ep = generate_bootstrap_secret();
        let mut view = MembershipView::empty(Uuid::new_v4(), "lab", 1);
        view.ensure_domain("lab");
        view.quorum_domain_default = "lab".into();
        let self_id = Uuid::new_v4();
        view.members.insert(
            self_id,
            MemberRecord {
                node_id: self_id,
                node_name: "n1".into(),
                status: MemberStatus::Ready,
                incarnation: 1,
                quorum_domain: "lab".into(),
                voter: true,
                labels: BTreeMap::from([("az".into(), "a".into())]),
            },
        );
        (view, vec![ep])
    }

    #[test]
    fn pending_not_member_or_voter() {
        let (mut view, epochs) = base_view();
        let secret = epochs[0].secret_bytes().unwrap();
        let id = Uuid::new_v4();
        let (ack, _) = handle_join(
            &mut view,
            &epochs,
            JoinRequest {
                node_id: id,
                node_name: "n2".into(),
                labels: BTreeMap::from([("az".into(), "b".into())]),
                internodes_address: "127.0.0.1:7000".into(),
                quorum_domain: "lab".into(),
                presented_secret: secret,
                join_token: None,
                replace_of: None,
            product_version: 1,
            },
            &["az".into()],
            |d| d == "lab",
        )
        .unwrap();
        assert_eq!(ack, JoinAck::Pending);
        assert!(view.is_pending(&id));
        assert!(!view.is_member(&id));
        assert!(!view.voting_members().iter().any(|m| m.node_id == id));
        assert!(!view.replica_targets().iter().any(|m| m.node_id == id));
    }

    #[test]
    fn unknown_domain_refused_when_lab_bootstrapped() {
        let (mut view, epochs) = base_view();
        let secret = epochs[0].secret_bytes().unwrap();
        let id = Uuid::new_v4();
        let domains = view.quorum_domains.clone();
        let (ack, _) = handle_join(
            &mut view,
            &epochs,
            JoinRequest {
                node_id: id,
                node_name: "n2".into(),
                labels: BTreeMap::from([("az".into(), "b".into())]),
                internodes_address: "127.0.0.1:7000".into(),
                quorum_domain: "default".into(),
                presented_secret: secret,
                join_token: None,
                replace_of: None,
            product_version: 1,
            },
            &["az".into()],
            |d| domains.contains(d),
        )
        .unwrap();
        assert!(matches!(ack, JoinAck::Refused { code } if code == "quorum_domain_unknown"));
        assert!(!view.is_pending(&id));
    }

    #[test]
    fn token_admits_without_pending() {
        let (mut view, epochs) = base_view();
        let secret = epochs[0].secret_bytes().unwrap();
        let (token, _) = mint_token(&mut view, "n2", None, crate::token::default_ttl()).unwrap();
        let id = Uuid::new_v4();
        let (ack, _) = handle_join(
            &mut view,
            &epochs,
            JoinRequest {
                node_id: id,
                node_name: "n2".into(),
                labels: BTreeMap::from([("az".into(), "b".into())]),
                internodes_address: "127.0.0.1:7000".into(),
                quorum_domain: "lab".into(),
                presented_secret: secret,
                join_token: Some(token.token_id),
                replace_of: None,
            product_version: 1,
            },
            &["az".into()],
            |_| true,
        )
        .unwrap();
        assert!(matches!(ack, JoinAck::Admitted { .. }));
        assert!(view.is_member(&id));
        assert!(!view.is_pending(&id));
    }

    #[test]
    fn missing_ladder_no_pending() {
        let (mut view, epochs) = base_view();
        let secret = epochs[0].secret_bytes().unwrap();
        let id = Uuid::new_v4();
        let (ack, _) = handle_join(
            &mut view,
            &epochs,
            JoinRequest {
                node_id: id,
                node_name: "n2".into(),
                labels: BTreeMap::new(),
                internodes_address: "127.0.0.1:7000".into(),
                quorum_domain: "lab".into(),
                presented_secret: secret,
                join_token: None,
                replace_of: None,
            product_version: 1,
            },
            &["az".into()],
            |_| true,
        )
        .unwrap();
        assert!(matches!(ack, JoinAck::Refused { code } if code == "ladder"));
        assert!(!view.is_pending(&id));
    }

    #[test]
    fn product_version_window_refuses_n_plus_2() {
        let (mut view, epochs) = base_view();
        let secret = epochs[0].secret_bytes().unwrap();
        let id = Uuid::new_v4();
        let (ack, _) = handle_join(
            &mut view,
            &epochs,
            JoinRequest {
                node_id: id,
                node_name: "n2".into(),
                labels: BTreeMap::from([("az".into(), "b".into())]),
                internodes_address: "127.0.0.1:7000".into(),
                quorum_domain: "lab".into(),
                presented_secret: secret,
                join_token: None,
                replace_of: None,
                product_version: 3,
            },
            &["az".into()],
            |_| true,
        )
        .unwrap();
        assert!(
            matches!(ack, JoinAck::Refused { code } if code == "product_version_window")
        );
    }
}

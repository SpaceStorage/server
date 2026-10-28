pub mod auth;

use crate::lifecycle::{self, NodeState};
use crate::reload;
use crate::Node;
use spacestorage_admin_proto::{
    AdminOp, EffectiveConfig, EntrypointStatus, ErrorBody, SettingReport, Status, StopResult,
    ThreadsReport,
};
use spacestorage_config::model::Transport;
use spacestorage_release_profile::ReleaseProfile;
use std::collections::HashSet;
use std::sync::Arc;

pub struct AdminService;

impl AdminService {
    pub async fn execute(
        node: &Arc<Node>,
        op: AdminOp,
    ) -> Result<serde_json::Value, ErrorBody> {
        match op {
            AdminOp::Status => Ok(serde_json::to_value(status(node)).unwrap()),
            AdminOp::Config => Ok(serde_json::to_value(config(node)).unwrap()),
            AdminOp::Threads => Ok(serde_json::to_value(threads(node)).unwrap()),
            AdminOp::Buffers => {
                let buffers = node.buffers.reports();
                let sum: u64 = buffers.iter().map(|b| b.capacity_bytes).sum();
                Ok(serde_json::json!({
                    "buffers": buffers,
                    "sum_capacity_bytes": sum,
                    "memory_available_bytes": null,
                    "exceeds_memory": false
                }))
            }
            AdminOp::Reload => {
                if node.state.get() != NodeState::Ready {
                    return Err(ErrorBody {
                        code: "invalid_state".into(),
                        message: "reload only when ready".into(),
                        details: serde_json::json!({"state": node.state.get().as_str()}),
                    });
                }
                let report = reload::reload(node).await?;
                Ok(serde_json::to_value(report).unwrap())
            }
            AdminOp::Stop { wait: _ } => {
                if node.state.get() == NodeState::Draining {
                    return Err(ErrorBody {
                        code: "invalid_state".into(),
                        message: "already draining".into(),
                        details: serde_json::json!({"state": "draining"}),
                    });
                }
                lifecycle::request_drain(node);
                Ok(serde_json::to_value(StopResult {
                    accepted: true,
                    state: "draining".into(),
                    drain_timeout_secs: node.config.load().drain_timeout.as_secs(),
                })
                .unwrap())
            }
            AdminOp::Controllers => controlplane_controllers(node).await,
            AdminOp::RaftStatus { group } => controlplane_raft_status(node, group).await,
            AdminOp::Leases { namespace } => controlplane_leases(node, namespace).await,
            AdminOp::VoterReplace { group, from, to } => {
                controlplane_voter_op(
                    node,
                    spacestorage_controlplane::membership::MembershipOp::Replace { from, to },
                    &group,
                )
                .await
            }
            AdminOp::VoterGrow { group, add } => {
                if add.len() != 2 {
                    return Err(ErrorBody {
                        code: "invalid_argument".into(),
                        message: "VoterGrow requires exactly two ids".into(),
                        details: serde_json::json!({}),
                    });
                }
                controlplane_voter_op(
                    node,
                    spacestorage_controlplane::membership::MembershipOp::Grow {
                        add: [add[0], add[1]],
                    },
                    &group,
                )
                .await
            }
            AdminOp::VoterShrink { group, remove } => {
                if remove.len() != 2 {
                    return Err(ErrorBody {
                        code: "invalid_argument".into(),
                        message: "VoterShrink requires exactly two ids".into(),
                        details: serde_json::json!({}),
                    });
                }
                controlplane_voter_op(
                    node,
                    spacestorage_controlplane::membership::MembershipOp::Shrink {
                        remove: [remove[0], remove[1]],
                    },
                    &group,
                )
                .await
            }
        }
    }
}

async fn require_cp(
    node: &Node,
) -> Result<Arc<spacestorage_controlplane::ControlPlane>, ErrorBody> {
    node.controlplane
        .read()
        .await
        .clone()
        .ok_or_else(|| ErrorBody {
            code: "unavailable".into(),
            message: "controlplane not started".into(),
            details: serde_json::json!({}),
        })
}

fn cp_err(e: spacestorage_controlplane::Error) -> ErrorBody {
    let code = match &e {
        spacestorage_controlplane::Error::NotLeader { .. } => "NotLeader",
        spacestorage_controlplane::Error::Minority { .. } => "Minority",
        spacestorage_controlplane::Error::Slice7Required { .. } => "Slice7Required",
        _ => "controlplane_error",
    };
    ErrorBody {
        code: code.into(),
        message: e.to_string(),
        details: serde_json::json!({}),
    }
}

async fn controlplane_controllers(node: &Node) -> Result<serde_json::Value, ErrorBody> {
    let cp = require_cp(node).await?;
    let view = cp.controllers_view().await;
    // Metadata reads via read-index when follower.
    if let Ok(g) = cp.cluster_group.try_lock() {
        if let Some(cg) = g.as_ref() {
            let _ = cg.read_index().allow_consistent_read().map_err(cp_err)?;
        }
    }
    Ok(serde_json::json!({
        "cluster_primary": view.cluster_primary,
        "cluster_secondaries": view.cluster_secondaries,
        "term": view.term,
        "commit_index": view.commit_index,
        "namespaces": view.namespaces.iter().map(|n| serde_json::json!({
            "namespace_id": n.namespace_id,
            "name": n.name,
            "primary": n.primary,
        })).collect::<Vec<_>>(),
    }))
}

async fn controlplane_raft_status(
    node: &Node,
    group: Option<String>,
) -> Result<serde_json::Value, ErrorBody> {
    let cp = require_cp(node).await?;
    let gid = match group.as_deref() {
        None | Some("cluster") => spacestorage_controlplane::group::GroupId::Cluster,
        Some(s) => {
            let id = uuid::Uuid::parse_str(s).map_err(|e| ErrorBody {
                code: "invalid_argument".into(),
                message: format!("group: {e}"),
                details: serde_json::json!({}),
            })?;
            spacestorage_controlplane::group::GroupId::Namespace(id)
        }
    };
    let st = cp.raft_status(gid).await.map_err(cp_err)?;
    Ok(serde_json::json!({
        "group": st.group.to_string(),
        "term": st.term,
        "commit_index": st.commit_index,
        "role": st.role,
        "leader": st.leader,
        "voters": st.voters,
        "learners": st.learners,
    }))
}

async fn controlplane_leases(
    node: &Node,
    namespace: uuid::Uuid,
) -> Result<serde_json::Value, ErrorBody> {
    let cp = require_cp(node).await?;
    let leases = cp.leases_view(namespace).map_err(cp_err)?;
    Ok(serde_json::to_value(leases).unwrap_or_else(|_| serde_json::json!([])))
}

async fn controlplane_voter_op(
    node: &Node,
    op: spacestorage_controlplane::membership::MembershipOp,
    _group: &str,
) -> Result<serde_json::Value, ErrorBody> {
    let cp = require_cp(node).await?;
    cp.voter_op(op).await.map_err(cp_err)?;
    Ok(serde_json::json!({"ok": true}))
}

fn entrypoint_statuses(node: &Node) -> Vec<EntrypointStatus> {
    let cfg = node.config.load();
    cfg.entrypoints
        .iter()
        .map(|ep| EntrypointStatus {
            name: ep.name.clone(),
            address: ep.address.clone(),
            port: ep.port,
            handler: ep.handler.clone(),
            transport: match ep.transport {
                Transport::Plaintext => "plaintext",
                Transport::Tls => "encrypted",
                Transport::Undeclared => "undeclared",
            }
            .into(),
            cert_expired: node.effective.cert_expired(&ep.name),
            connections_active: 0,
        })
        .collect()
}

fn status(node: &Node) -> Status {
    Status {
        node_name: node.config.load().node_name.clone(),
        state: node.state.get().as_str().into(),
        uptime_seconds: node.started_at.elapsed().as_secs(),
        version: env!("CARGO_PKG_VERSION").into(),
        threads: ThreadsReport {
            total: node.worker_threads,
            busy: node.stats.busy(),
            source: node.threads_source.clone(),
            available_cores: std::thread::available_parallelism()
                .ok()
                .map(|n| n.get() as u32),
        },
        entrypoints: entrypoint_statuses(node),
        buffers: node.buffers.reports(),
        drain_timed_out: node.stats.drain_timed_out(),
        release_profile: Some(ReleaseProfile::FirstBinary.as_str().into()),
        handlers: Some(node.handler_names()),
    }
}

fn settings_report(node: &Node) -> Vec<SettingReport> {
    let cfg = node.config.load();
    let pending: HashSet<String> = node.effective.pending_restart().into_iter().collect();
    let mark = |name: &str| -> Option<serde_json::Value> {
        if pending.contains(name) {
            Some(serde_json::Value::Bool(true))
        } else {
            None
        }
    };
    vec![
        SettingReport {
            setting: "runtime.threads".into(),
            value: serde_json::json!(node.worker_threads),
            reload_class: "restart_required".into(),
            pending_restart: mark("runtime.threads"),
        },
        SettingReport {
            setting: "runtime.drain_timeout".into(),
            value: serde_json::json!(cfg.drain_timeout.as_secs()),
            reload_class: "live".into(),
            pending_restart: mark("runtime.drain_timeout"),
        },
        SettingReport {
            setting: "log.level".into(),
            value: serde_json::json!(cfg.log_level),
            reload_class: "live".into(),
            pending_restart: mark("log.level"),
        },
        SettingReport {
            setting: "log.format".into(),
            value: serde_json::json!(cfg.log_format),
            reload_class: "restart_required".into(),
            pending_restart: mark("log.format"),
        },
        SettingReport {
            setting: "admin.token_file".into(),
            value: serde_json::json!(cfg.admin_token_file),
            reload_class: "live".into(),
            pending_restart: mark("admin.token_file"),
        },
        SettingReport {
            setting: "entrypoints".into(),
            value: serde_json::to_value(&cfg.entrypoints).unwrap_or(serde_json::Value::Null),
            reload_class: "restart_required".into(),
            pending_restart: mark("entrypoints"),
        },
        SettingReport {
            setting: "buffers".into(),
            value: serde_json::to_value(&cfg.buffers).unwrap_or(serde_json::Value::Null),
            reload_class: "live".into(),
            pending_restart: mark("buffers"),
        },
    ]
}

fn config(node: &Node) -> EffectiveConfig {
    EffectiveConfig {
        node_name: node.config.load().node_name.clone(),
        threads: node.worker_threads,
        threads_source: node.threads_source.clone(),
        drain_timeout_secs: node.config.load().drain_timeout.as_secs(),
        log_level: node.config.load().log_level.clone(),
        log_format: node.config.load().log_format.clone(),
        handlers: node.handler_names(),
        entrypoints: entrypoint_statuses(node),
        buffers: node.buffers.reports(),
        pending_restart: node.effective.pending_restart(),
        settings: settings_report(node),
        release_profile: Some(ReleaseProfile::FirstBinary.as_str().into()),
    }
}

fn threads(node: &Node) -> ThreadsReport {
    ThreadsReport {
        total: node.worker_threads,
        busy: node.stats.busy(),
        source: node.threads_source.clone(),
        available_cores: std::thread::available_parallelism()
            .ok()
            .map(|n| n.get() as u32),
    }
}

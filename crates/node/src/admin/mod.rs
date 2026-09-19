pub mod auth;

use crate::lifecycle::NodeState;
use crate::reload;
use crate::Node;
use spacestorage_admin_proto::{
    AdminOp, EffectiveConfig, EntrypointStatus, ErrorBody, Status, StopResult,
    ThreadsReport,
};
use spacestorage_config::model::Transport;
use spacestorage_release_profile::ReleaseProfile;
use std::sync::atomic::Ordering;
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
                node.drain_flag.store(true, Ordering::SeqCst);
                node.state.begin_drain();
                node.cancel.cancel();
                Ok(serde_json::to_value(StopResult {
                    accepted: true,
                    state: "draining".into(),
                    drain_timeout_secs: node.config.load().drain_timeout.as_secs(),
                })
                .unwrap())
            }
        }
    }
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
            cert_expired: false,
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

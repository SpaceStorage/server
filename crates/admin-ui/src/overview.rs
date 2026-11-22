//! Cluster overview snapshot composer — `GET /v1/ui/overview`.
//! Contract: `specs/018-admin-ui-modernization/contracts/ui-read-models.md`.

use crate::ui_time::rfc3339_now;
use serde::{Deserialize, Serialize};

pub const DEGRADED_CAP: usize = 20;
pub const REPLICA_NAMED_CAP: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberLine {
    pub node_id: String,
    pub node_name: String,
    pub readiness: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LagLine {
    pub namespace: String,
    pub container: String,
    pub replica: String,
    pub lag_ms: u64,
}

#[derive(Debug, Clone, Default)]
pub struct OverviewComposeInput {
    pub source: &'static str,
    pub cluster_ready: bool,
    pub members_ready: u32,
    pub members_not_ready: u32,
    pub members_pending: u32,
    pub members_unknown: u32,
    pub degraded: Vec<MemberLine>,
    pub controller_available: bool,
    pub raft: &'static str,
    pub namespace_count: u32,
    pub container_count: u32,
    pub container_count_complete: bool,
    pub replicas_healthy: u64,
    pub replicas_degraded: u64,
    pub replicas_lagging: u64,
    pub lagging_named: Vec<LagLine>,
    pub jobs_active: u32,
    pub jobs_failed: u32,
    pub jobs_recently_completed: u32,
}

pub fn compose_overview(input: &OverviewComposeInput) -> serde_json::Value {
    let (degraded, degraded_omitted) = cap_lines(&input.degraded, DEGRADED_CAP);
    let (named, named_omitted) = cap_lag(&input.lagging_named, REPLICA_NAMED_CAP);
    let readiness = if input.cluster_ready {
        "ready"
    } else {
        "not_ready"
    };
    serde_json::json!({
        "generated_at": rfc3339_now(),
        "source": input.source,
        "readiness": { "status": "ok", "value": readiness },
        "members": {
            "status": "ok",
            "ready": input.members_ready,
            "not_ready": input.members_not_ready,
            "pending": input.members_pending,
            "unknown": input.members_unknown,
            "degraded": degraded,
            "degraded_omitted": degraded_omitted,
        },
        "controller": {
            "status": "ok",
            "available": input.controller_available,
            "raft": input.raft,
        },
        "counts": {
            "status": "ok",
            "namespaces": input.namespace_count,
            "containers": input.container_count,
            "container_count_complete": input.container_count_complete,
        },
        "replicas": {
            "status": "ok",
            "healthy": input.replicas_healthy,
            "degraded": input.replicas_degraded,
            "lagging": input.replicas_lagging,
            "named": named,
            "named_omitted": named_omitted,
        },
        "jobs": {
            "status": "ok",
            "active": input.jobs_active,
            "failed": input.jobs_failed,
            "recently_completed": input.jobs_recently_completed,
        },
    })
}

fn cap_lines(rows: &[MemberLine], cap: usize) -> (Vec<serde_json::Value>, u32) {
    let omitted = rows.len().saturating_sub(cap) as u32;
    let slice: Vec<_> = rows
        .iter()
        .take(cap)
        .map(|m| {
            serde_json::json!({
                "node_id": m.node_id,
                "node_name": m.node_name,
                "readiness": m.readiness,
            })
        })
        .collect();
    (slice, omitted)
}

fn cap_lag(rows: &[LagLine], cap: usize) -> (Vec<serde_json::Value>, u32) {
    let omitted = rows.len().saturating_sub(cap) as u32;
    let slice: Vec<_> = rows
        .iter()
        .take(cap)
        .map(|r| {
            serde_json::json!({
                "namespace": r.namespace,
                "container": r.container,
                "replica": r.replica,
                "lag_ms": r.lag_ms,
            })
        })
        .collect();
    (slice, omitted)
}

/// Trim overview panels after compose-time namespace filtering (legacy guard).
pub fn filter_overview_for_namespaces(input: &mut OverviewComposeInput, allowed: &[String]) {
    if allowed.is_empty() {
        return;
    }
    input.lagging_named
        .retain(|r| allowed.iter().any(|n| n == &r.namespace));
    input.replicas_lagging = input.lagging_named.len() as u64;
}
//! Compose `/v1/ui/*` read models from the running node (018).

use crate::Node;
use crate::handler::live_map::placement_describe_snapshot;
use spacestorage_admin_ui::jobs_view::JobRow;
use spacestorage_admin_ui::namespaces::{NamespaceContainerRow, NamespaceListRow};
use spacestorage_admin_ui::nodes::{NodeDetailView, NodeListRow};
use spacestorage_migrate::JobRecord;
use spacestorage_observability::filter_namespace;
use spacestorage_observability::sample::SeriesSnapshot;
use spacestorage_admin_ui::overview::{LagLine, MemberLine, OverviewComposeInput};
use spacestorage_admin_ui::namespaces::DescriptorScanBudget;
use spacestorage_admin_ui::UiPrincipal;
use spacestorage_membership::events::MemberStatus;
use spacestorage_placement::{ReplicaHealth, ReplicaLag};
use spacestorage_admin_ui::shell::ShellSection;
use spacestorage_tenancy::QuotaUnit;
use spacestorage_types::StorageModeChoice;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

fn namespace_allowed(ns: &str, filter: Option<&[String]>) -> bool {
    match filter {
        None => true,
        Some(allowed) => allowed.iter().any(|n| n == ns),
    }
}

fn quota_json(quotas: &[spacestorage_tenancy::QuotaSpec]) -> Option<serde_json::Value> {
    quotas
        .iter()
        .find(|q| q.unit == QuotaUnit::Bytes)
        .map(|q| serde_json::json!({ "bytes": q.limit }))
}

pub async fn build_overview_input(
    node: &Arc<Node>,
    namespace_filter: Option<&[String]>,
) -> OverviewComposeInput {
    let mut input = OverviewComposeInput {
        source: "node-local",
        cluster_ready: matches!(node.state.get(), crate::lifecycle::NodeState::Ready),
        ..Default::default()
    };
    if let Some(cp) = node.controlplane.read().await.as_ref() {
        input.source = "controller";
        input.controller_available = true;
        input.raft = "leader";
        let namespaces = cp.tenancy_namespaces();
        input.namespace_count = namespaces
            .iter()
            .filter(|r| !r.deleted && namespace_allowed(&r.name, namespace_filter))
            .count() as u32;
    }
    if let Some(m) = node.membership.read().await.as_ref() {
        if let Some(view) = m.view_snapshot() {
            input.source = "controller";
            for (id, rec) in &view.members {
                let readiness = match rec.status {
                    MemberStatus::Ready => {
                        input.members_ready += 1;
                        "ready"
                    }
                    MemberStatus::Draining => {
                        input.members_not_ready += 1;
                        "not_ready"
                    }
                };
                if readiness == "not_ready" || readiness == "pending" {
                    input.degraded.push(MemberLine {
                        node_id: id.to_string(),
                        node_name: rec.node_name.clone(),
                        readiness: readiness.into(),
                    });
                }
            }
            input.members_pending = view.pending.len() as u32;
            for (_, pj) in &view.pending {
                input.degraded.push(MemberLine {
                    node_id: pj.node_id.to_string(),
                    node_name: pj.node_name.clone(),
                    readiness: "pending".into(),
                });
            }
        }
    }
    if input.members_ready + input.members_not_ready + input.members_pending == 0 {
        let cfg = node.config.load();
        input.degraded.push(MemberLine {
            node_id: cfg.node_name.clone(),
            node_name: cfg.node_name.clone(),
            readiness: if input.cluster_ready {
                "ready".into()
            } else {
                "not_ready".into()
            },
        });
        if input.cluster_ready {
            input.members_ready = 1;
        } else {
            input.members_not_ready = 1;
        }
    }
    let describe = placement_describe_snapshot(node).await;
    let cat = node.catalog.read().unwrap();
    let mut budget = DescriptorScanBudget::new();
    let mut container_count = 0u32;
    for c in cat.list() {
        let ns = c.namespace.as_str();
        if !namespace_allowed(ns, namespace_filter) {
            continue;
        }
        if budget.take(1) == 0 {
            input.container_count_complete = false;
            break;
        }
        container_count += 1;
        let rows = describe.describe_container(ns, c.name.as_str());
        for r in rows {
            match r.health {
                ReplicaHealth::InSync => input.replicas_healthy += 1,
                ReplicaHealth::Behind => {
                    input.replicas_degraded += 1;
                    input.replicas_lagging += 1;
                    input.lagging_named.push(LagLine {
                        namespace: ns.to_string(),
                        container: c.name.as_str().to_string(),
                        replica: r
                            .id
                            .clone()
                            .unwrap_or_else(|| r.node_name.clone()),
                        lag_ms: r.lag.duration_ms,
                    });
                }
                ReplicaHealth::Unavailable | ReplicaHealth::ContentLost => {
                    input.replicas_degraded += 1
                }
                _ => input.replicas_degraded += 1,
            }
        }
    }
    input.container_count = container_count;
    input.container_count_complete =
        input.container_count_complete && !budget.at_cap();
    if input.namespace_count == 0 {
        input.namespace_count = node
            .tenancy
            .registry
            .lock()
            .list()
            .iter()
            .filter(|r| !r.deleted && namespace_allowed(&r.name, namespace_filter))
            .count() as u32;
    }
    input.jobs_active = node
        .jobs
        .list()
        .iter()
        .filter(|j| {
            matches!(
                j.status,
                spacestorage_migrate::JobStatus::Running | spacestorage_migrate::JobStatus::Starting
            )
        })
        .count() as u32;
    input
}

pub async fn build_namespace_list_rows(
    node: &Arc<Node>,
    namespace_filter: Option<&[String]>,
    q: Option<&str>,
) -> (Vec<NamespaceListRow>, bool) {
    let describe = placement_describe_snapshot(node).await;
    let registry = node.tenancy.registry.lock();
    let mut names: Vec<_> = registry
        .list()
        .into_iter()
        .filter(|r| !r.deleted)
        .filter(|r| namespace_allowed(&r.name, namespace_filter))
        .map(|r| r.name.clone())
        .collect();
    if let Some(q) = q {
        if !q.is_empty() {
            names.retain(|n| n.contains(q));
        }
    }
    names.sort();
    let cat = node.catalog.read().unwrap();
    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    let mut complete = true;
    let mut budget = DescriptorScanBudget::new();
    for c in cat.list() {
        let ns = c.namespace.as_str();
        if !namespace_allowed(ns, namespace_filter) {
            continue;
        }
        if budget.take(1) == 0 {
            complete = false;
            break;
        }
        *counts.entry(ns.to_string()).or_default() += 1;
    }
    let mut rows = Vec::new();
    for name in names {
        let Some(rec) = registry.get_by_name(&name) else {
            continue;
        };
        let mut health = "unknown";
        for c in cat.list() {
            if c.namespace.as_str() != name {
                continue;
            }
            let replica_rows = describe.describe_container(name.as_str(), c.name.as_str());
            if replica_rows
                .iter()
                .any(|r| matches!(r.health, ReplicaHealth::Behind))
            {
                health = "degraded";
                break;
            } else if !replica_rows.is_empty() {
                health = "ok";
            }
        }
        rows.push(NamespaceListRow {
            name,
            container_count: counts.get(rec.name.as_str()).copied().unwrap_or(0),
            container_count_complete: complete && !budget.at_cap(),
            quota: quota_json(&rec.quotas),
            health: health.into(),
        });
    }
    (rows, complete)
}

pub async fn build_namespace_detail(
    node: &Arc<Node>,
    name: &str,
) -> Option<(NamespaceListRow, serde_json::Value, Vec<NamespaceContainerRow>)> {
    let rec = {
        let registry = node.tenancy.registry.lock();
        let rec = registry.get_by_name(name)?;
        if rec.deleted {
            return None;
        }
        rec.clone()
    };
    let describe = placement_describe_snapshot(node).await;
    let cat = node.catalog.read().unwrap();
    let mut budget = DescriptorScanBudget::new();
    let mut container_count = 0u32;
    let mut complete = true;
    let mut health = "unknown";
    let mut containers = Vec::new();
    for c in cat.list() {
        if c.namespace.as_str() != name {
            continue;
        }
        if budget.take(1) == 0 {
            complete = false;
            break;
        }
        container_count += 1;
        let replica_rows = describe.describe_container(name, c.name.as_str());
        let replicas_degraded = if replica_rows.is_empty() {
            None
        } else {
            Some(
                replica_rows
                    .iter()
                    .filter(|r| !matches!(r.health, ReplicaHealth::InSync))
                    .count() as u32,
            )
        };
        let shards = if replica_rows.is_empty() {
            None
        } else {
            Some(replica_rows.len() as u32)
        };
        if replica_rows
            .iter()
            .any(|r| matches!(r.health, ReplicaHealth::Behind))
        {
            health = "degraded";
        } else if !replica_rows.is_empty() {
            health = "ok";
        }
        containers.push(NamespaceContainerRow {
            name: c.name.as_str().to_string(),
            model: c.model.machine_name().to_string(),
            mode: match c.mode {
                StorageModeChoice::Memory => "memory",
                StorageModeChoice::Persistent => "persistent",
                StorageModeChoice::Hybrid => "hybrid",
            }
            .into(),
            shards,
            replicas_degraded,
        });
    }
    let summary = NamespaceListRow {
        name: name.to_string(),
        container_count,
        container_count_complete: complete && !budget.at_cap(),
        quota: quota_json(&rec.quotas),
        health: health.into(),
    };
    let record = serde_json::to_value(rec).unwrap_or(serde_json::json!({}));
    Some((summary, record, containers))
}

pub async fn build_node_list_rows(
    node: &Arc<Node>,
    namespace_filter: Option<&[String]>,
) -> (Vec<NodeListRow>, &'static str) {
    let describe = placement_describe_snapshot(node).await;
    let allowed_hosts = if let Some(allowed) = namespace_filter {
        let cat = node.catalog.read().unwrap();
        let mut hosts = HashSet::new();
        for c in cat.list() {
            if !namespace_allowed(c.namespace.as_str(), Some(allowed)) {
                continue;
            }
            for r in describe.describe_container(c.namespace.as_str(), c.name.as_str()) {
                hosts.insert(r.node_name);
            }
        }
        Some(hosts)
    } else {
        None
    };
    let mut rows = Vec::new();
    let mut source = "node-local";
    if let Some(m) = node.membership.read().await.as_ref() {
        if let Some(view) = m.view_snapshot() {
            source = "controller";
            for (id, rec) in &view.members {
                let readiness = match rec.status {
                    MemberStatus::Ready => "ready",
                    MemberStatus::Draining => "not_ready",
                };
                rows.push(NodeListRow {
                    node_id: id.to_string(),
                    node_name: rec.node_name.clone(),
                    readiness: readiness.into(),
                    controller_role: if rec.voter {
                        Some("voter".into())
                    } else {
                        Some("non_voter".into())
                    },
                });
            }
            for (_, pj) in &view.pending {
                rows.push(NodeListRow {
                    node_id: pj.node_id.to_string(),
                    node_name: pj.node_name.clone(),
                    readiness: "pending".into(),
                    controller_role: None,
                });
            }
        }
    }
    if rows.is_empty() {
        let cfg = node.config.load();
        rows.push(NodeListRow {
            node_id: cfg.node_name.clone(),
            node_name: cfg.node_name.clone(),
            readiness: if matches!(node.state.get(), crate::lifecycle::NodeState::Ready) {
                "ready".into()
            } else {
                "not_ready".into()
            },
            controller_role: None,
        });
    }
    if let Some(hosts) = allowed_hosts {
        rows.retain(|r| hosts.contains(&r.node_name));
    }
    (rows, source)
}

pub async fn build_node_detail(
    node: &Arc<Node>,
    node_id: &str,
) -> Option<(NodeListRow, NodeDetailView)> {
    let (rows, _) = build_node_list_rows(node, None).await;
    let row = rows
        .into_iter()
        .find(|r| r.node_id == node_id || r.node_name == node_id)?;
    let cfg = node.config.load();
    let is_local = row.node_name == cfg.node_name || row.node_id == cfg.node_name;
    let state = Some(node.state.get().as_str().to_string());
    let (worker_threads, buffers) = if is_local {
        let threads = crate::admin::AdminService::execute(node, spacestorage_admin_proto::AdminOp::Threads)
            .await
            .ok();
        let buffers = crate::admin::AdminService::execute(node, spacestorage_admin_proto::AdminOp::Buffers)
            .await
            .ok();
        (threads, buffers)
    } else {
        (None, None)
    };
    let labels = if is_local {
        Some(serde_json::to_value(&cfg.labels).unwrap_or(serde_json::json!({})))
    } else {
        None
    };
    let reported_by = if is_local {
        Some(cfg.node_name.clone())
    } else {
        None
    };
    let detail = NodeDetailView {
        node_id: row.node_id.clone(),
        node_name: row.node_name.clone(),
        readiness: row.readiness.clone(),
        controller_role: row.controller_role.clone(),
        state,
        worker_threads,
        buffers,
        labels,
        reported_by,
    };
    Some((row, detail))
}

pub fn principal_from_ui(p: &UiPrincipal) -> (String, Vec<String>) {
    if p.cluster_admin {
        ("cluster".into(), vec![])
    } else {
        ("namespace".into(), p.namespace_admin.clone())
    }
}

pub fn section_refusal(path: &str, principal: &UiPrincipal) -> Option<&'static str> {
    let (scope, ns) = if principal.cluster_admin {
        ("cluster", principal.namespace_admin.as_slice())
    } else {
        ("namespace", principal.namespace_admin.as_slice())
    };
    let section = ShellSection::from_path(path)?;
    let monitoring_ns = spacestorage_admin_ui::shell::monitoring_namespace_from_path(path);
    if !spacestorage_admin_ui::shell::section_allowed_for_scope(section, scope) {
        if !(section == ShellSection::Monitoring && monitoring_ns.is_some()) {
            return Some("This section is not available for your scope.");
        }
    }
    spacestorage_admin_ui::shell::check_namespace_path(path, scope, ns).err()
}

/// Test helper: seed lagging replica describe for conformance.
pub async fn seed_lagging_replica(
    node: &Arc<Node>,
    namespace: &str,
    container: &str,
    lag_ms: u64,
) {
    seed_lagging_replica_on(node, namespace, container, "db-c", lag_ms).await;
}

pub async fn seed_lagging_replica_on(
    node: &Arc<Node>,
    namespace: &str,
    container: &str,
    replica_host: &str,
    lag_ms: u64,
) {
    use crate::handler::live_map::append_placement_event;
    use spacestorage_placement::PlacementEvent;
    let event = PlacementEvent::ReplicaHealth {
        id: uuid::Uuid::new_v4(),
        origin: "conformance".into(),
        namespace: namespace.into(),
        container: container.into(),
        node_name: replica_host.into(),
        health: ReplicaHealth::Behind,
        lag: ReplicaLag::new(Duration::from_millis(lag_ms), 0),
        replica_id: Some("r2".into()),
    };
    let _ = append_placement_event(node, event).await;
}

fn series_to_json(s: &SeriesSnapshot) -> serde_json::Value {
    let labels: serde_json::Map<String, serde_json::Value> = s
        .labels
        .inner()
        .iter()
        .map(|(k, v)| (k.clone(), serde_json::json!(v)))
        .collect();
    let mut obj = serde_json::json!({
        "name": s.name,
        "labels": labels,
    });
    if let Some(v) = s.counter {
        obj["value"] = serde_json::json!(v);
    } else if let Some(v) = s.gauge {
        obj["value"] = serde_json::json!(v);
    } else if s.hist_count.is_some() {
        obj["buckets"] = serde_json::json!({
            "bounds": s.hist_bounds,
            "counts": s.hist_counts,
            "sum": s.hist_sum,
            "count": s.hist_count,
        });
    }
    obj
}

pub fn build_monitoring_panel(
    node: &Node,
    namespace: Option<&str>,
) -> (
    &'static str,
    Option<String>,
    Vec<serde_json::Value>,
    Vec<String>,
) {
    let snap = node.metrics.registry().snapshot();
    let filtered = match namespace {
        Some(ns) => filter_namespace(&snap, ns),
        None => snap,
    };
    let series: Vec<_> = filtered.iter().map(series_to_json).collect();
    let want: Vec<String> = spacestorage_observability::all_families()
        .iter()
        .map(|f| f.name.to_string())
        .collect();
    let scope = if namespace.is_some() {
        "namespace"
    } else {
        "cluster"
    };
    (scope, namespace.map(str::to_string), series, want)
}

fn job_allowed_target(target: &str, filter: Option<&[String]>) -> bool {
    match filter {
        None => true,
        Some(allowed) => {
            let ns = target.split('/').next().unwrap_or("");
            allowed.iter().any(|n| n == ns)
        }
    }
}

pub fn job_to_row(j: &JobRecord) -> JobRow {
    let target = j
        .target_internal_name
        .clone()
        .unwrap_or_else(|| "—".into());
    JobRow {
        id: j.id.to_string(),
        kind: j.kind.as_str().into(),
        target,
        status: j.status.as_str().into(),
        paused: j.paused,
        progress: serde_json::to_value(&j.progress).unwrap_or(serde_json::json!({})),
        failure: j.error.clone(),
    }
}

pub fn build_job_rows(node: &Node, namespace_filter: Option<&[String]>) -> Vec<JobRow> {
    node.jobs
        .list()
        .iter()
        .map(job_to_row)
        .filter(|r| job_allowed_target(&r.target, namespace_filter))
        .collect()
}

pub fn find_job_row(node: &Node, id: &str, namespace_filter: Option<&[String]>) -> Option<JobRow> {
    let jid = uuid::Uuid::parse_str(id).ok()?;
    let job = node.jobs.status(jid).ok()?;
    let row = job_to_row(&job);
    if job_allowed_target(&row.target, namespace_filter) {
        Some(row)
    } else {
        None
    }
}
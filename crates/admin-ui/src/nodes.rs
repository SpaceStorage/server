//! Node roster/detail composers — `GET /v1/ui/nodes*`.

use crate::shell::{self, PageParams};
use crate::ui_time::rfc3339_now;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct NodeListRow {
    pub node_id: String,
    pub node_name: String,
    pub readiness: String,
    pub controller_role: Option<String>,
}

fn page_start(cursor: &Option<String>, sort_keys: &[String]) -> usize {
    let Some(c) = cursor.as_ref().and_then(|c| shell::decode_cursor(c)) else {
        return 0;
    };
    sort_keys
        .iter()
        .position(|k| k.as_str() > c.as_str())
        .unwrap_or(sort_keys.len())
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeDetailView {
    pub node_id: String,
    pub node_name: String,
    pub readiness: String,
    pub controller_role: Option<String>,
    pub state: Option<String>,
    pub worker_threads: Option<serde_json::Value>,
    pub buffers: Option<serde_json::Value>,
    pub labels: Option<serde_json::Value>,
    pub reported_by: Option<String>,
}

pub fn compose_node_detail(row: &NodeListRow, detail: &NodeDetailView) -> serde_json::Value {
    serde_json::json!({
        "generated_at": rfc3339_now(),
        "node_id": row.node_id,
        "node_name": row.node_name,
        "readiness": row.readiness,
        "controller_role": row.controller_role,
        "state": detail.state,
        "worker_threads": detail.worker_threads,
        "buffers": detail.buffers,
        "labels": detail.labels,
        "reported_by": detail.reported_by,
    })
}

pub fn compose_nodes_list(
    params: &PageParams,
    source: &str,
    mut rows: Vec<NodeListRow>,
) -> serde_json::Value {
    rows.sort_by(|a, b| a.node_name.cmp(&b.node_name));
    let keys: Vec<String> = rows.iter().map(|r| r.node_name.clone()).collect();
    let start = page_start(&params.cursor, &keys);
    let limit = params.limit as usize;
    let mut page: Vec<NodeListRow> = rows.into_iter().skip(start).take(limit + 1).collect();
    let has_more = page.len() > limit;
    if has_more {
        page.truncate(limit);
    }
    let mut sort_keys: Vec<String> = page.iter().map(|r| r.node_name.clone()).collect();
    let next_cursor = shell::page_close_on_byte_budget(&mut page, &mut sort_keys, |r| {
        serde_json::json!({
            "node_id": r.node_id,
            "node_name": r.node_name,
            "readiness": r.readiness,
            "controller_role": r.controller_role,
        })
    })
    .or_else(|| {
        if has_more {
            page.last().map(|r| shell::encode_cursor(&r.node_name))
        } else {
            None
        }
    });
    serde_json::json!({
        "generated_at": rfc3339_now(),
        "source": source,
        "limit": params.limit,
        "next_cursor": next_cursor,
        "nodes": page,
    })
}
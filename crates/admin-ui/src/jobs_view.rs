//! Job progress pages — `GET /v1/ui/jobs*`.

use crate::shell::{self, PageParams};
use crate::ui_time::rfc3339_now;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct JobRow {
    pub id: String,
    pub kind: String,
    pub target: String,
    pub status: String,
    pub paused: bool,
    pub progress: serde_json::Value,
    pub failure: Option<String>,
}

pub fn compose_jobs_list(params: &PageParams, mut rows: Vec<JobRow>) -> serde_json::Value {
    rows.sort_by(|a, b| b.id.cmp(&a.id));
    let keys: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
    let start = page_start(&params.cursor, &keys);
    let limit = params.limit as usize;
    let mut page: Vec<JobRow> = rows.into_iter().skip(start).take(limit + 1).collect();
    let has_more = page.len() > limit;
    if has_more {
        page.truncate(limit);
    }
    let next_cursor = if has_more {
        page.last().map(|r| shell::encode_cursor(&r.id))
    } else {
        None
    };
    serde_json::json!({
        "generated_at": rfc3339_now(),
        "limit": params.limit,
        "next_cursor": next_cursor,
        "jobs": page,
    })
}

pub fn compose_job_detail(row: &JobRow) -> serde_json::Value {
    serde_json::json!({
        "generated_at": rfc3339_now(),
        "id": row.id,
        "kind": row.kind,
        "target": row.target,
        "status": row.status,
        "paused": row.paused,
        "progress": row.progress,
        "failure": row.failure,
    })
}

fn page_start(cursor: &Option<String>, sort_keys: &[String]) -> usize {
    let Some(c) = cursor.as_ref().and_then(|c| shell::decode_cursor(c)) else {
        return 0;
    };
    sort_keys
        .iter()
        .position(|k| k.as_str() < c.as_str())
        .unwrap_or(sort_keys.len())
}
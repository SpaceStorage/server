//! Audit read-only page — `GET /v1/ui/audit`.

use crate::shell::PageParams;
use crate::ui_time::rfc3339_now;

pub fn compose_audit_page(params: &PageParams, entries: Vec<serde_json::Value>) -> serde_json::Value {
    serde_json::json!({
        "generated_at": rfc3339_now(),
        "limit": params.limit,
        "next_cursor": serde_json::Value::Null,
        "entries": entries,
    })
}

pub fn merge_audit_page(
    params: &PageParams,
    limit: u32,
    next_cursor: Option<String>,
    entries: Vec<serde_json::Value>,
) -> serde_json::Value {
    let _ = params;
    serde_json::json!({
        "generated_at": rfc3339_now(),
        "limit": limit,
        "next_cursor": next_cursor,
        "entries": entries,
    })
}
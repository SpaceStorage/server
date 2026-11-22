//! Namespace list/detail composers — `GET /v1/ui/namespaces*`.
//! Contract: `specs/018-admin-ui-modernization/contracts/ui-read-models.md`.

use crate::shell::{self, PageParams};
use crate::ui_time::rfc3339_now;
use serde::Serialize;

/// Bounded descriptor scan helper (R6). Drops catalog lock before await at call sites.
pub struct DescriptorScanBudget {
    pub remaining: usize,
}

impl DescriptorScanBudget {
    pub fn new() -> Self {
        Self {
            remaining: shell::DESCRIPTOR_SCAN_CAP,
        }
    }

    pub fn take(&mut self, n: usize) -> usize {
        let take = n.min(self.remaining);
        self.remaining -= take;
        take
    }

    pub fn at_cap(&self) -> bool {
        self.remaining == 0
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct NamespaceListRow {
    pub name: String,
    pub container_count: u32,
    pub container_count_complete: bool,
    pub quota: Option<serde_json::Value>,
    pub health: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NamespaceContainerRow {
    pub name: String,
    pub model: String,
    pub mode: String,
    pub shards: Option<u32>,
    pub replicas_degraded: Option<u32>,
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

/// Compose namespace list page (R5 + 256 KiB row budget).
pub fn compose_namespace_list(
    params: &PageParams,
    mut rows: Vec<NamespaceListRow>,
) -> serde_json::Value {
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    let keys: Vec<String> = rows.iter().map(|r| r.name.clone()).collect();
    let start = page_start(&params.cursor, &keys);
    let limit = params.limit as usize;
    let mut page: Vec<NamespaceListRow> = rows.into_iter().skip(start).take(limit + 1).collect();
    let has_more = page.len() > limit;
    if has_more {
        page.truncate(limit);
    }
    let mut sort_keys: Vec<String> = page.iter().map(|r| r.name.clone()).collect();
    let next_cursor = shell::page_close_on_byte_budget(&mut page, &mut sort_keys, |r| {
        serde_json::json!({
            "name": r.name,
            "container_count": r.container_count,
            "container_count_complete": r.container_count_complete,
            "quota": r.quota,
            "health": r.health,
        })
    })
    .or_else(|| {
        if has_more {
            page.last().map(|r| shell::encode_cursor(&r.name))
        } else {
            None
        }
    });
    serde_json::json!({
        "generated_at": rfc3339_now(),
        "limit": params.limit,
        "next_cursor": next_cursor,
        "namespaces": page,
    })
}

/// Namespace detail with paged container inventory.
pub fn compose_namespace_detail(
    summary: &NamespaceListRow,
    record: &serde_json::Value,
    params: &PageParams,
    mut containers: Vec<NamespaceContainerRow>,
) -> serde_json::Value {
    containers.sort_by(|a, b| a.name.cmp(&b.name));
    let keys: Vec<String> = containers.iter().map(|c| c.name.clone()).collect();
    let start = page_start(&params.cursor, &keys);
    let limit = params.limit as usize;
    let mut page: Vec<NamespaceContainerRow> = containers
        .into_iter()
        .skip(start)
        .take(limit.saturating_add(1))
        .collect();
    let has_more = page.len() > limit;
    if has_more {
        page.truncate(limit);
    }
    let mut sort_keys: Vec<String> = page.iter().map(|c| c.name.clone()).collect();
    let next = shell::page_close_on_byte_budget(&mut page, &mut sort_keys, |c| {
        serde_json::json!({
            "name": c.name,
            "model": c.model,
            "mode": c.mode,
            "shards": c.shards,
            "replicas_degraded": c.replicas_degraded,
        })
    });
    let next_cursor = next.or_else(|| {
        if has_more {
            page.last().map(|r| shell::encode_cursor(&r.name))
        } else {
            None
        }
    });
    serde_json::json!({
        "generated_at": rfc3339_now(),
        "name": summary.name,
        "container_count": summary.container_count,
        "container_count_complete": summary.container_count_complete,
        "quota": summary.quota,
        "health": summary.health,
        "record": record,
        "containers": {
            "limit": params.limit,
            "next_cursor": next_cursor,
            "items": page,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_cap() {
        let mut b = DescriptorScanBudget::new();
        assert_eq!(b.take(5_000), 5_000);
        assert_eq!(b.take(10_000), 5_000);
        assert!(b.at_cap());
    }
}
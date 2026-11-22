//! Monitoring panels — `GET /v1/ui/monitoring*`.

use crate::ui_time::rfc3339_now;

pub fn compose_monitoring_stub() -> serde_json::Value {
    compose_monitoring_panel("cluster", None, &[], &[])
}

/// Build panel: present series pass through; missing family names land in `unavailable` (never value 0).
pub fn compose_monitoring_panel(
    scope: &str,
    namespace: Option<&str>,
    present: &[serde_json::Value],
    want: &[String],
) -> serde_json::Value {
    let have: std::collections::BTreeSet<&str> = present
        .iter()
        .filter_map(|s| s.get("name").and_then(|v| v.as_str()))
        .collect();
    let unavailable: Vec<_> = want
        .iter()
        .filter(|n| !have.contains(n.as_str()))
        .map(|name| {
            serde_json::json!({
                "name": name,
                "reason": "series absent",
            })
        })
        .collect();
    serde_json::json!({
        "generated_at": rfc3339_now(),
        "scope": scope,
        "namespace": namespace,
        "series": present,
        "unavailable": unavailable,
    })
}
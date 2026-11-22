//! Contract fixture shape gates (018 T006).

use std::path::PathBuf;

fn fixture(name: &str) -> serde_json::Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../specs/018-admin-ui-modernization/contracts/fixtures")
        .join(name);
    let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    serde_json::from_str(&text).expect("json")
}

#[test]
fn overview_degraded_fixture_keys() {
    let j = fixture("overview-degraded.json");
    assert!(j.get("generated_at").is_some());
    assert!(j.get("members").and_then(|m| m.get("degraded")).is_some());
    assert!(j.get("replicas").and_then(|r| r.get("named")).is_some());
}

#[test]
fn namespace_page_fixture_keys() {
    let j = fixture("namespace-page.json");
    assert_eq!(j.get("limit").and_then(|v| v.as_u64()), Some(50));
    assert!(j.get("namespaces").is_some());
}

#[test]
fn audit_page_fixture_keys() {
    let j = fixture("audit-page.json");
    let row = j.get("entries").and_then(|e| e.get(0)).expect("one row");
    assert!(row.get("principal").is_some());
    assert!(row.get("action").is_some());
    assert!(row.get("target").is_some());
}
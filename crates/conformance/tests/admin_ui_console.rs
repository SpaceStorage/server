//! Admin UI console conformance (009 US2) — feature `complete-product`.

#![cfg(feature = "complete-product")]

use spacestorage_admin_ui::{ConsoleAuthz, ConsoleService, APPLICATION_UI};

#[test]
fn namespace_admin_create_insert_browse() {
    let svc = ConsoleService::new(true);
    let mut authz = ConsoleAuthz {
        namespace_admin: vec!["acme".into()],
        create_namespaces: vec!["acme".into()],
        read_containers: vec!["acme/events".into()],
        ..Default::default()
    };
    let created = svc
        .create_container(
            &authz,
            br#"{"namespace":"acme","name":"events","type":"log_stream"}"#,
        )
        .unwrap();
    assert_eq!(created["application"], APPLICATION_UI);
    svc.insert_row("acme", "events", "1", serde_json::json!({"v":1}));
    authz.read_containers.push("acme/events".into());
    let page = svc
        .query(
            &authz,
            br#"{"namespace":"acme","container":"events","limit":10}"#,
        )
        .unwrap();
    assert_eq!(page["application"], APPLICATION_UI);
    assert_eq!(page["engine"], "planner");
    assert!(page["rows"].as_array().unwrap().len() >= 1);
}

#[test]
fn namespace_admin_cluster_config_forbidden() {
    let svc = ConsoleService::new(true);
    let authz = ConsoleAuthz {
        namespace_admin: vec!["acme".into()],
        ..Default::default()
    };
    let err = svc
        .config(
            &authz,
            br#"{"scope":"cluster","settings":{"x":1}}"#,
        )
        .unwrap_err();
    assert_eq!(err.code(), "forbidden");
}

#[test]
fn oversized_browse_payload_too_large() {
    let svc = ConsoleService::new(true);
    let authz = ConsoleAuthz {
        cluster_admin: true,
        ..Default::default()
    };
    // Fill beyond MAX_PAGE_BYTES via many large rows.
    let big = "x".repeat(200_000);
    for i in 0..10 {
        svc.insert_row(
            "acme",
            "blob",
            &i.to_string(),
            serde_json::json!(big),
        );
    }
    let err = svc
        .query(
            &authz,
            br#"{"namespace":"acme","container":"blob","limit":1000}"#,
        )
        .unwrap_err();
    assert_eq!(err.code(), "payload_too_large");
}

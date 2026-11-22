//! Overview composer vs fixture shape (018 T032).

use spacestorage_admin_ui::overview::{
    compose_overview, LagLine, MemberLine, OverviewComposeInput, DEGRADED_CAP, REPLICA_NAMED_CAP,
};

#[test]
fn caps_degraded_and_replicas() {
    let mut input = OverviewComposeInput::default();
    input.source = "controller";
    input.degraded = (0..25)
        .map(|i| MemberLine {
            node_id: format!("id-{i}"),
            node_name: format!("n{i}"),
            readiness: "not_ready".into(),
        })
        .collect();
    input.lagging_named = (0..25)
        .map(|i| LagLine {
            namespace: "acme".into(),
            container: "orders".into(),
            replica: format!("r{i}"),
            lag_ms: 100,
        })
        .collect();
    let j = compose_overview(&input);
    let members = &j["members"];
    assert_eq!(members["degraded"].as_array().unwrap().len(), DEGRADED_CAP);
    assert_eq!(members["degraded_omitted"].as_u64(), Some(5));
    let replicas = &j["replicas"];
    assert_eq!(replicas["named"].as_array().unwrap().len(), REPLICA_NAMED_CAP);
    assert_eq!(replicas["named_omitted"].as_u64(), Some(5));
}

#[test]
fn fixture_degraded_shape() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../specs/018-admin-ui-modernization/contracts/fixtures/overview-degraded.json");
    let fixture: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let input = OverviewComposeInput {
        source: "controller",
        members_ready: 2,
        members_not_ready: 1,
        members_pending: 1,
        degraded: vec![MemberLine {
            node_id: "018f2c6e-7b1a-7c3d-8e4f-1a2b3c4d5e6f".into(),
            node_name: "n3".into(),
            readiness: "not_ready".into(),
        }],
        controller_available: true,
        raft: "leader",
        namespace_count: 50,
        container_count: 500,
        container_count_complete: true,
        replicas_lagging: 1,
        lagging_named: vec![LagLine {
            namespace: "acme".into(),
            container: "orders".into(),
            replica: "r2".into(),
            lag_ms: 4200,
        }],
        jobs_active: 1,
        ..Default::default()
    };
    let composed = compose_overview(&input);
    for key in ["source", "members", "replicas", "counts", "jobs"] {
        assert!(composed.get(key).is_some(), "missing {key}");
        assert!(fixture.get(key).is_some(), "fixture missing {key}");
    }
    assert_eq!(composed["source"], "controller");
}
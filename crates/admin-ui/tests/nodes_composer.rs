//! Nodes list composer (018 T046).

use spacestorage_admin_ui::nodes::{compose_nodes_list, NodeListRow};
use spacestorage_admin_ui::shell::PageParams;

#[test]
fn pending_join_readiness() {
    let rows = vec![NodeListRow {
        node_id: "id-p".into(),
        node_name: "joiner".into(),
        readiness: "pending".into(),
        controller_role: None,
    }];
    let j = compose_nodes_list(&PageParams::default(), "controller", rows);
    assert_eq!(
        j["nodes"][0]["readiness"],
        "pending"
    );
}

#[test]
fn unknown_never_coerced_to_ready() {
    let rows = vec![NodeListRow {
        node_id: "id-u".into(),
        node_name: "peer".into(),
        readiness: "unknown".into(),
        controller_role: None,
    }];
    let j = compose_nodes_list(&PageParams::default(), "controller", rows);
    assert_eq!(j["nodes"][0]["readiness"], "unknown");
    assert_ne!(j["nodes"][0]["readiness"], "ready");
}

#[test]
fn peer_worker_threads_null_on_list() {
    let j = compose_nodes_list(&PageParams::default(), "controller", vec![]);
    assert!(j.get("nodes").is_some());
    assert!(j.get("worker_threads").is_none());
}

#[test]
fn nodes_list_next_cursor_when_more_rows() {
    let rows: Vec<NodeListRow> = (0..3)
        .map(|i| NodeListRow {
            node_id: format!("id-{i}"),
            node_name: format!("node-{i:02}"),
            readiness: "ready".into(),
            controller_role: None,
        })
        .collect();
    let j = compose_nodes_list(
        &PageParams {
            limit: 2,
            cursor: None,
        },
        "controller",
        rows,
    );
    assert_eq!(j["nodes"].as_array().unwrap().len(), 2);
    let cur = j.get("next_cursor").and_then(|c| c.as_str()).expect("next_cursor");
    let j2 = compose_nodes_list(
        &PageParams {
            limit: 2,
            cursor: Some(cur.into()),
        },
        "controller",
        (0..3)
            .map(|i| NodeListRow {
                node_id: format!("id-{i}"),
                node_name: format!("node-{i:02}"),
                readiness: "ready".into(),
                controller_role: None,
            })
            .collect(),
    );
    assert_eq!(j2["nodes"].as_array().unwrap().len(), 1);
    assert!(j2.get("next_cursor").and_then(|c| c.as_str()).is_none());
}
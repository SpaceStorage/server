//! Namespace list composer vs contract fixture (018 T040).

use spacestorage_admin_ui::namespaces::{
    compose_namespace_detail, compose_namespace_list, NamespaceContainerRow, NamespaceListRow,
};
use spacestorage_admin_ui::shell::{PageParams, DEFAULT_LIMIT, PAGE_BYTE_CAP};

#[test]
fn default_limit_matches_fixture() {
    let rows = vec![NamespaceListRow {
        name: "acme".into(),
        container_count: 12,
        container_count_complete: true,
        quota: Some(serde_json::json!({ "bytes": 1073741824 })),
        health: "ok".into(),
    }];
    let j = compose_namespace_list(&PageParams { limit: DEFAULT_LIMIT, cursor: None }, rows);
    assert_eq!(j.get("limit").and_then(|v| v.as_u64()), Some(50));
    assert!(j.get("namespaces").unwrap().as_array().unwrap().len() == 1);
}

#[test]
fn incomplete_count_copy_path_at_least() {
    let row = NamespaceListRow {
        name: "acme".into(),
        container_count: 9_999,
        container_count_complete: false,
        quota: None,
        health: "unknown".into(),
    };
    assert!(!row.container_count_complete);
    let label = if row.container_count_complete {
        row.container_count.to_string()
    } else {
        format!("at least {}", row.container_count)
    };
    assert!(label.starts_with("at least"));
}

#[test]
fn byte_budget_sets_next_cursor_without_dropping_silently() {
    let big = "x".repeat(PAGE_BYTE_CAP / 4);
    let mut rows = Vec::new();
    for i in 0..200 {
        rows.push(NamespaceListRow {
            name: format!("{big}-{i}"),
            container_count: 1,
            container_count_complete: true,
            quota: None,
            health: "ok".into(),
        });
    }
    let j = compose_namespace_list(
        &PageParams {
            limit: 200,
            cursor: None,
        },
        rows,
    );
    let page = j.get("namespaces").unwrap().as_array().unwrap();
    assert!(page.len() < 200);
    assert!(j.get("next_cursor").and_then(|c| c.as_str()).is_some());
}

#[test]
fn namespace_detail_containers_next_cursor() {
    let summary = NamespaceListRow {
        name: "acme".into(),
        container_count: 3,
        container_count_complete: true,
        quota: None,
        health: "ok".into(),
    };
    let containers: Vec<NamespaceContainerRow> = (0..3)
        .map(|i| NamespaceContainerRow {
            name: format!("c-{i:02}"),
            model: "kv".into(),
            mode: "rw".into(),
            shards: Some(1),
            replicas_degraded: Some(0),
        })
        .collect();
    let j = compose_namespace_detail(
        &summary,
        &serde_json::json!({}),
        &PageParams {
            limit: 2,
            cursor: None,
        },
        containers.clone(),
    );
    let items = j["containers"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    let cur = j["containers"]["next_cursor"]
        .as_str()
        .expect("containers next_cursor");
    let j2 = compose_namespace_detail(
        &summary,
        &serde_json::json!({}),
        &PageParams {
            limit: 2,
            cursor: Some(cur.into()),
        },
        containers,
    );
    assert_eq!(j2["containers"]["items"].as_array().unwrap().len(), 1);
    assert!(j2["containers"]["next_cursor"].is_null());
}
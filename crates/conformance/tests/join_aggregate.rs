//! SC-009 — join + aggregate (`query-distributed`).

#![cfg(feature = "query-distributed")]

use spacestorage_query::{
    AggFn, CancelToken, JoinKind, LogicalRequest, PlannerEngine, QueryOptions,
};
use spacestorage_types::ContainerCatalog;
use std::sync::{Arc, RwLock};

#[test]
fn join_and_group_by() {
    let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
    let eng = PlannerEngine::with_distributed(Arc::clone(&cat), true);
    for sql in [
        "CREATE TABLE a (id text, k text)",
        "CREATE TABLE b (id text, k text)",
        "INSERT INTO a VALUES ('1', 'x')",
        "INSERT INTO b VALUES ('10', 'x')",
        "INSERT INTO b VALUES ('11', 'y')",
    ] {
        eng.execute_blocking(
            "ns",
            "s",
            "postgresql",
            LogicalRequest::AdHocSql { sql: sql.into() },
            QueryOptions::default(),
            CancelToken::new(),
        )
        .unwrap();
    }
    let joined = eng
        .execute_blocking(
            "ns",
            "s",
            "postgresql",
            LogicalRequest::Join {
                left: "a".into(),
                right: "b".into(),
                kind: JoinKind::Inner,
                left_key: "k".into(),
                right_key: "k".into(),
            },
            QueryOptions::default(),
            CancelToken::new(),
        )
        .unwrap();
    assert_eq!(joined.rows.len(), 1);

    let agg = eng
        .execute_blocking(
            "ns",
            "s",
            "postgresql",
            LogicalRequest::Aggregate {
                container: "a".into(),
                func: AggFn::Count,
                group_by: Some("k".into()),
                column: None,
                filter: None,
            },
            QueryOptions::default(),
            CancelToken::new(),
        )
        .unwrap();
    assert!(!agg.rows.is_empty());
}

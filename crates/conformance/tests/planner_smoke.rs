//! SC-001 / SC-012 — planner smoke through shared PlannerEngine.

use spacestorage_compat::DialectProfile;
use spacestorage_handler_postgresql::execute_sql;
use spacestorage_query::{
    CancelToken, ExecutionStage, LogicalRequest, PlannerEngine, QueryOptions,
};
use spacestorage_types::ContainerCatalog;
use std::sync::{Arc, RwLock};

#[test]
fn pg_crud_through_planner_engine_label() {
    let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
    let eng = PlannerEngine::new(Arc::clone(&cat));
    eng.execute_blocking(
        "ns",
        "s",
        "postgresql",
        LogicalRequest::AdHocSql {
            sql: "CREATE TABLE t (id text, v text)".into(),
        },
        QueryOptions::default(),
        CancelToken::new(),
    )
    .unwrap();
    eng.execute_blocking(
        "ns",
        "s",
        "postgresql",
        LogicalRequest::AdHocSql {
            sql: "INSERT INTO t VALUES ('1', 'a')".into(),
        },
        QueryOptions::default(),
        CancelToken::new(),
    )
    .unwrap();
    let sel = eng
        .execute_blocking(
            "ns",
            "s",
            "postgresql",
            LogicalRequest::AdHocSql {
                sql: "SELECT * FROM t".into(),
            },
            QueryOptions::default(),
            CancelToken::new(),
        )
        .unwrap();
    assert_eq!(sel.rows.len(), 1);
    let rec = eng.records_snapshot().last().cloned().unwrap();
    assert_eq!(rec.engine, "planner");
    assert_eq!(rec.stage, ExecutionStage::Done);
}

#[test]
fn first_binary_begin_copy_not_supported() {
    let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
    let p = DialectProfile::FirstBinary;
    for sql in ["BEGIN", "COPY t FROM STDIN"] {
        let e = execute_sql(p, &cat, "ns", sql).unwrap_err();
        assert_eq!(e.sqlstate, "0A000");
    }
}

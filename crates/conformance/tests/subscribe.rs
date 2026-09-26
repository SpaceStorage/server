//! SC-010 — subscribe / job wait (`query-distributed`).

#![cfg(feature = "query-distributed")]

use spacestorage_compat::DialectProfile;
use spacestorage_handler_postgresql::execute_sql;
use spacestorage_query::{
    engines::subscribe, CancelToken, JobState, PlannerEngine,
};
use spacestorage_types::ContainerCatalog;
use std::sync::{Arc, RwLock};

#[test]
fn job_submit_and_listen_refused() {
    let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
    let eng = PlannerEngine::with_distributed(Arc::clone(&cat), true);
    let id = subscribe::submit(
        &eng.jobs(),
        Arc::clone(&cat),
        "ns",
        "demo",
        "SELECT 1",
        CancelToken::new(),
    )
    .unwrap();
    let sub = eng.jobs().get(id).unwrap();
    assert!(matches!(
        sub.state,
        JobState::Succeeded | JobState::Running | JobState::Accepted
    ));
    assert!(eng.jobs().cancel(id) || matches!(sub.state, JobState::Succeeded));

    let e = execute_sql(DialectProfile::CompleteProduct, &cat, "ns", "LISTEN foo").unwrap_err();
    assert_eq!(e.sqlstate, "0A000");
}

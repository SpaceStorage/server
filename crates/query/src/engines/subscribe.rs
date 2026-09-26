//! Subscribe / async job lifecycle (005 FR-014).

use crate::cancel::CancelToken;
use crate::catalog_exec::{execute_adhoc_sql, QueryResult, SharedCatalog};
use crate::error::ExecError;
use crate::subscribe::{JobRegistry, JobState, Subscription};
use uuid::Uuid;

/// Submit `sql` as async job; returns Accepted exec_id.
pub fn submit(
    jobs: &JobRegistry,
    catalog: SharedCatalog,
    namespace: &str,
    principal: &str,
    sql: &str,
    cancel: CancelToken,
) -> Result<Uuid, ExecError> {
    let sub = Subscription::accepted(namespace, principal);
    let id = jobs.submit(sub);
    jobs.set_state(id, JobState::Running);
    // Run synchronously for unit/conformance (background spawn is node wiring).
    if cancel.is_cancelled() || jobs.get(id).map(|j| j.state == JobState::Cancelled).unwrap_or(false)
    {
        jobs.set_state(id, JobState::Cancelled);
        return Ok(id);
    }
    match execute_adhoc_sql(&catalog, namespace, sql) {
        Ok(r) => {
            jobs.succeed(id, r.tag);
        }
        Err(e) => {
            jobs.fail(id, e.to_string());
        }
    }
    Ok(id)
}

pub fn wait(jobs: &JobRegistry, exec_id: Uuid) -> Result<Subscription, ExecError> {
    jobs.get(exec_id)
        .ok_or_else(|| ExecError::Msg(format!("unknown_job:{exec_id}")))
}

pub fn cancel(jobs: &JobRegistry, exec_id: Uuid, token: &CancelToken) -> bool {
    token.cancel();
    jobs.cancel(exec_id)
}

pub fn result_from_job(sub: &Subscription) -> QueryResult {
    QueryResult {
        tag: sub
            .result_tag
            .clone()
            .unwrap_or_else(|| format!("{:?}", sub.state)),
        columns: vec!["exec_id".into(), "state".into()],
        rows: vec![vec![
            Some(sub.exec_id.to_string()),
            Some(format!("{:?}", sub.state)),
        ]],
    }
}

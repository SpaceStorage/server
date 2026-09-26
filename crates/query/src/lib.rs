//! Shared query planner/executor (feature 005).
//!
//! Repository crate name is `spacestorage-query` (`crates/query`). Spec tasks
//! historically said `crates/exec`; this crate is that engine.

pub mod admission;
pub mod cancel;
pub mod catalog_exec;
pub mod engine;
pub mod error;
pub mod local;
pub mod options;
pub mod planner;
pub mod rank;
pub mod record;
pub mod request;
pub mod schedule;
pub mod spill;
pub mod subscribe;
pub mod txn;

#[cfg(feature = "query-distributed")]
pub mod engines;

pub use admission::{AdmissionController, AdmissionLimits, AdmissionToken};
pub use cancel::CancelToken;
pub use catalog_exec::{QueryResult, SharedCatalog};
pub use engine::{lower_sql, PlannerEngine, QueryEngine};
pub use error::{ExecError, PartUnavailable};
pub use local::LocalEngine;
pub use options::{Concurrency, IsolationLevel, QueryOptions, Sourced};
pub use planner::{EngineKind, LogicalPlan, PhysicalPlan, Task};
pub use record::{ExecutionRecord, ExecutionStage};
pub use request::{AggFn, CopyFormat, JoinKind, LogicalRequest};
pub use subscribe::{JobRegistry, JobState, SharedJobRegistry, Subscription};
pub use txn::{SharedTxnRegistry, Transaction, TxnRegistry, TxnState};

/// Legacy stub surface kept for early unit tests.
#[derive(Debug, Clone)]
pub struct QueryEngineStub {
    pub max_memory_bytes: u64,
}

impl Default for QueryEngineStub {
    fn default() -> Self {
        Self {
            max_memory_bytes: 64 * 1024 * 1024,
        }
    }
}

impl QueryEngineStub {
    pub fn plan_sql(&self, sql: &str) -> Result<(), ExecError> {
        let u = sql.trim_start().to_ascii_uppercase();
        if u.starts_with("BEGIN")
            || u.starts_with("COMMIT")
            || u.starts_with("ROLLBACK")
            || u.starts_with("COPY")
        {
            return Err(ExecError::NotSupported("feature_not_supported".into()));
        }
        Ok(())
    }

    pub fn admit(&self, estimated_bytes: u64) -> Result<(), ExecError> {
        if estimated_bytes > self.max_memory_bytes {
            Err(ExecError::Admission {
                limit: "memory".into(),
                current: estimated_bytes,
                max: self.max_memory_bytes,
            })
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_copy_refused_stub() {
        let eng = QueryEngineStub::default();
        assert!(matches!(
            eng.plan_sql("BEGIN"),
            Err(ExecError::NotSupported(_))
        ));
        assert!(matches!(
            eng.plan_sql("COPY t FROM STDIN"),
            Err(ExecError::NotSupported(_))
        ));
        assert!(eng.plan_sql("SELECT 1").is_ok());
    }
}

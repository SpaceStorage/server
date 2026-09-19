//! PostgreSQL first-binary dialect (002) — lowers into QueryEngine.

use spacestorage_query::{QueryEngine, QueryError};

pub const WIRE_VERSION: &str = "3.0";
pub const FEATURE_NOT_SUPPORTED: &str = "0A000";

pub struct PostgresqlHandler {
    pub engine: QueryEngine,
}

impl Default for PostgresqlHandler {
    fn default() -> Self {
        Self {
            engine: QueryEngine::default(),
        }
    }
}

impl PostgresqlHandler {
    pub fn exec_sql(&self, sql: &str) -> Result<(), (String, String)> {
        self.engine.plan_sql(sql).map_err(|e| match e {
            QueryError::FeatureNotSupported => {
                (FEATURE_NOT_SUPPORTED.into(), "feature_not_supported".into())
            }
            other => ("XX000".into(), other.to_string()),
        })
    }
}

//! Shared query execution — first-binary CRUD + admission (005 US1/US2/US4).

use thiserror::Error;

#[derive(Debug, Error)]
pub enum QueryError {
    #[error("feature_not_supported")]
    FeatureNotSupported,
    #[error("admission_rejected")]
    AdmissionRejected,
    #[error("{0}")]
    Msg(String),
}

#[derive(Debug, Clone)]
pub struct QueryEngine {
    pub max_memory_bytes: u64,
}

impl Default for QueryEngine {
    fn default() -> Self {
        Self {
            max_memory_bytes: 64 * 1024 * 1024,
        }
    }
}

impl QueryEngine {
    pub fn plan_sql(&self, sql: &str) -> Result<(), QueryError> {
        let u = sql.trim_start().to_ascii_uppercase();
        if u.starts_with("BEGIN") || u.starts_with("COMMIT") || u.starts_with("ROLLBACK") || u.starts_with("COPY") {
            return Err(QueryError::FeatureNotSupported);
        }
        Ok(())
    }

    pub fn admit(&self, estimated_bytes: u64) -> Result<(), QueryError> {
        if estimated_bytes > self.max_memory_bytes {
            Err(QueryError::AdmissionRejected)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_copy_refused() {
        let eng = QueryEngine::default();
        assert!(matches!(eng.plan_sql("BEGIN"), Err(QueryError::FeatureNotSupported)));
        assert!(matches!(eng.plan_sql("COPY t FROM STDIN"), Err(QueryError::FeatureNotSupported)));
        assert!(eng.plan_sql("SELECT 1").is_ok());
    }
}

//! PostgreSQL SQL — classify via `015`, execute via shared `PlannerEngine` (005).

use spacestorage_compat::{classify_pg_verb, ClassifyOutcome, DialectProfile};
use spacestorage_query::{lower_sql, CancelToken, PlannerEngine, QueryOptions};
use spacestorage_types::ContainerCatalog;
use std::sync::{Arc, RwLock};

use crate::wire::FEATURE_NOT_SUPPORTED;

#[derive(Debug, Clone)]
pub struct ExecResult {
    pub tag: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
}

#[derive(Debug, Clone)]
pub struct ExecError {
    pub sqlstate: String,
    pub message: String,
}

pub type SharedCatalog = Arc<RwLock<ContainerCatalog>>;

fn first_verb(sql: &str) -> String {
    sql.trim_start()
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_end_matches(';')
        .to_ascii_uppercase()
}

fn refuse(outcome: ClassifyOutcome, verb: &str) -> ExecError {
    let (sqlstate, message) = match outcome {
        ClassifyOutcome::MustNot(e) => match e {
            spacestorage_compat::CompatError::BeginNotInProfile
            | spacestorage_compat::CompatError::CopyNotInProfile
            | spacestorage_compat::CompatError::CursorNotInProfile => (
                FEATURE_NOT_SUPPORTED.into(),
                format!("feature_not_supported: {verb}"),
            ),
            other => (
                FEATURE_NOT_SUPPORTED.into(),
                format!("feature_not_supported: {} ({})", verb, other.code()),
            ),
        },
        ClassifyOutcome::Must => (
            "XX000".into(),
            format!("internal: expected MustNot for {verb}"),
        ),
    };
    ExecError { sqlstate, message }
}

/// Execute one SQL statement through the shared `PlannerEngine` (005).
pub fn execute_sql(
    profile: DialectProfile,
    catalog: &SharedCatalog,
    namespace: &str,
    sql: &str,
) -> Result<ExecResult, ExecError> {
    execute_sql_session(profile, catalog, namespace, "default", sql)
}

pub fn execute_sql_session(
    profile: DialectProfile,
    catalog: &SharedCatalog,
    namespace: &str,
    session: &str,
    sql: &str,
) -> Result<ExecResult, ExecError> {
    let sql = sql.trim().trim_end_matches(';');
    if sql.is_empty() {
        return Ok(ExecResult {
            tag: "EMPTY".into(),
            columns: vec![],
            rows: vec![],
        });
    }

    let verb = first_verb(sql);
    if matches!(verb.as_str(), "LISTEN" | "NOTIFY") {
        return Err(ExecError {
            sqlstate: FEATURE_NOT_SUPPORTED.into(),
            message: format!("feature_not_supported: {verb}"),
        });
    }

    let outcome = classify_pg_verb(profile, &verb);
    if !outcome.is_must() {
        return Err(refuse(outcome, &verb));
    }

    if matches!(verb.as_str(), "DECLARE" | "FETCH" | "CLOSE") {
        let upper = sql.to_ascii_uppercase();
        if upper.contains("WITH HOLD") || upper.contains(" SCROLL") {
            return Err(ExecError {
                sqlstate: FEATURE_NOT_SUPPORTED.into(),
                message: "feature_not_supported: WITH HOLD/SCROLL".into(),
            });
        }
        // Forward-only cursor: treat DECLARE as no-op portal; FETCH as SELECT *
        if verb == "DECLARE" {
            return Ok(ExecResult {
                tag: "DECLARE CURSOR".into(),
                columns: vec![],
                rows: vec![],
            });
        }
        if verb == "CLOSE" {
            return Ok(ExecResult {
                tag: "CLOSE CURSOR".into(),
                columns: vec![],
                rows: vec![],
            });
        }
        // FETCH — return empty success for portal drain
        return Ok(ExecResult {
            tag: "FETCH 0".into(),
            columns: vec![],
            rows: vec![],
        });
    }

    let distributed = matches!(profile, DialectProfile::CompleteProduct);
    let engine = PlannerEngine::with_distributed(Arc::clone(catalog), distributed);
    let req = lower_sql(profile, sql).map_err(|e| ExecError {
        sqlstate: FEATURE_NOT_SUPPORTED.into(),
        message: e.to_string(),
    })?;

    match engine.execute_blocking(
        namespace,
        session,
        "postgresql",
        req,
        QueryOptions::default(),
        CancelToken::new(),
    ) {
        Ok(r) => Ok(ExecResult {
            tag: r.tag,
            columns: r.columns,
            rows: r.rows,
        }),
        Err(e) => {
            let sqlstate = match e.code() {
                "not_supported" | "snapshot_unsupported" | "isolation" => FEATURE_NOT_SUPPORTED,
                "retryable_txn" => "40001",
                _ => "XX000",
            };
            Err(ExecError {
                sqlstate: sqlstate.into(),
                message: e.to_string(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_copy_are_0a000() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let p = DialectProfile::FirstBinary;
        for sql in ["BEGIN", "COMMIT", "ROLLBACK", "COPY t FROM STDIN"] {
            let e = execute_sql(p, &cat, "ns", sql).unwrap_err();
            assert_eq!(e.sqlstate, FEATURE_NOT_SUPPORTED);
        }
    }

    #[test]
    fn crud_autocommit() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let p = DialectProfile::FirstBinary;
        execute_sql(p, &cat, "ns", "CREATE TABLE t (id text, v text)").unwrap();
        execute_sql(p, &cat, "ns", "INSERT INTO t VALUES ('1', 'a')").unwrap();
        let sel = execute_sql(p, &cat, "ns", "SELECT * FROM t").unwrap();
        assert_eq!(sel.rows.len(), 1);
        execute_sql(p, &cat, "ns", "UPDATE t SET v = 'b' WHERE id = '1'").unwrap();
        let sel = execute_sql(p, &cat, "ns", "SELECT * FROM t WHERE id = '1'").unwrap();
        assert_eq!(sel.rows[0][1].as_deref(), Some("b"));
        execute_sql(p, &cat, "ns", "DELETE FROM t WHERE id = '1'").unwrap();
        execute_sql(p, &cat, "ns", "DROP TABLE t").unwrap();
    }

    #[test]
    fn complete_product_begin_commit() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let p = DialectProfile::CompleteProduct;
        execute_sql_session(p, &cat, "ns", "s1", "BEGIN").unwrap();
        execute_sql_session(p, &cat, "ns", "s1", "CREATE TABLE t (id text, v text)").unwrap();
        execute_sql_session(p, &cat, "ns", "s1", "INSERT INTO t VALUES ('1', 'a')").unwrap();
        execute_sql_session(p, &cat, "ns", "s1", "COMMIT").unwrap();
        let sel = execute_sql(p, &cat, "ns", "SELECT * FROM t").unwrap();
        assert_eq!(sel.rows.len(), 1);
    }

    #[test]
    fn serializable_refused() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let p = DialectProfile::CompleteProduct;
        let e = execute_sql(p, &cat, "ns", "BEGIN ISOLATION LEVEL SERIALIZABLE").unwrap_err();
        assert_eq!(e.sqlstate, FEATURE_NOT_SUPPORTED);
    }
}

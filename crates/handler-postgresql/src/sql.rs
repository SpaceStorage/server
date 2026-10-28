//! PostgreSQL SQL — classify via `015`, execute via shared `PlannerEngine` (005).

use spacestorage_compat::{classify_pg_verb, ClassifyOutcome, DialectProfile, Limits};
use spacestorage_query::{
    lower_sql, CancelToken, CursorPortal, PlannerEngine, QueryOptions, QueryResult,
};
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

fn map_query_result(r: QueryResult) -> ExecResult {
    ExecResult {
        tag: r.tag,
        columns: r.columns,
        rows: r.rows,
    }
}

fn map_exec_err(e: spacestorage_query::ExecError) -> ExecError {
    let sqlstate = match e.code() {
        "not_supported" | "snapshot_unsupported" | "isolation" => FEATURE_NOT_SUPPORTED,
        "cancelled" => "57014", // query_canceled
        "retryable_txn" => "40001",
        "limit_exceeded" | "admission_rejected" => "54000",
        _ => "XX000",
    };
    ExecError {
        sqlstate: sqlstate.into(),
        message: e.to_string(),
    }
}

/// Parse `DECLARE name [NO SCROLL] CURSOR [WITHOUT HOLD] FOR <query>`.
fn parse_declare(sql: &str) -> Result<(String, String), ExecError> {
    let upper = sql.to_ascii_uppercase();
    if upper.contains("WITH HOLD") || upper.contains(" SCROLL") {
        return Err(ExecError {
            sqlstate: FEATURE_NOT_SUPPORTED.into(),
            message: "feature_not_supported: WITH HOLD/SCROLL".into(),
        });
    }
    let parts: Vec<&str> = sql.split_whitespace().collect();
    // DECLARE <name> …
    let name = parts.get(1).copied().unwrap_or("c").to_string();
    let for_pos = upper
        .find(" FOR ")
        .ok_or_else(|| ExecError {
            sqlstate: FEATURE_NOT_SUPPORTED.into(),
            message: "DECLARE requires FOR <query>".into(),
        })?;
    let query = sql[for_pos + 5..].trim().to_string();
    Ok((name, query))
}

fn parse_fetch(sql: &str) -> Result<(String, usize), ExecError> {
    // FETCH [FORWARD] [n] FROM name | FETCH name
    let upper = sql.to_ascii_uppercase();
    if upper.contains("BACKWARD") {
        return Err(ExecError {
            sqlstate: FEATURE_NOT_SUPPORTED.into(),
            message: "feature_not_supported: FETCH BACKWARD".into(),
        });
    }
    let parts: Vec<&str> = sql.split_whitespace().collect();
    let mut n = 1usize;
    let mut name = String::from("c");
    if let Some(from_i) = parts.iter().position(|p| p.eq_ignore_ascii_case("FROM")) {
        name = parts.get(from_i + 1).copied().unwrap_or("c").to_string();
        // FETCH FORWARD n FROM name — n before FROM
        for p in &parts[1..from_i] {
            if let Ok(v) = p.parse::<usize>() {
                n = v;
            }
        }
    } else if parts.len() >= 2 {
        name = parts[1].to_string();
    }
    Ok((name, n))
}

fn parse_close(sql: &str) -> String {
    sql.split_whitespace()
        .nth(1)
        .unwrap_or("c")
        .trim_end_matches(';')
        .to_string()
}

fn run_cursor(
    profile: DialectProfile,
    catalog: &SharedCatalog,
    namespace: &str,
    session: &str,
    sql: &str,
    verb: &str,
    cancel: &CancelToken,
) -> Result<ExecResult, ExecError> {
    let upper = sql.to_ascii_uppercase();
    if upper.contains("WITH HOLD") || upper.contains(" SCROLL") {
        return Err(ExecError {
            sqlstate: FEATURE_NOT_SUPPORTED.into(),
            message: "feature_not_supported: WITH HOLD/SCROLL".into(),
        });
    }

    let engine = PlannerEngine::with_distributed(
        Arc::clone(catalog),
        matches!(profile, DialectProfile::CompleteProduct),
    );
    let txn = engine.txns().get(session);
    let open = txn
        .as_ref()
        .map(|t| t.state == spacestorage_query::TxnState::Open)
        .unwrap_or(false);
    if !open {
        return Err(ExecError {
            sqlstate: FEATURE_NOT_SUPPORTED.into(),
            message: "feature_not_supported: cursor requires open transaction".into(),
        });
    }

    match verb {
        "DECLARE" => {
            let (name, query) = parse_declare(sql)?;
            cancel.check().map_err(map_exec_err)?;
            let req = lower_sql(profile, &query).map_err(map_exec_err)?;
            let result = engine
                .execute_blocking(
                    namespace,
                    session,
                    "postgresql",
                    req,
                    QueryOptions::default(),
                    cancel.clone(),
                )
                .map_err(map_exec_err)?;
            engine
                .txns()
                .with_mut(session, |t| {
                    t.portals.insert(
                        name.clone(),
                        CursorPortal {
                            name: name.clone(),
                            columns: result.columns.clone(),
                            rows: result.rows.clone(),
                            pos: 0,
                        },
                    );
                    Ok(())
                })
                .map_err(map_exec_err)?;
            Ok(ExecResult {
                tag: "DECLARE CURSOR".into(),
                columns: vec![],
                rows: vec![],
            })
        }
        "FETCH" => {
            let (name, n) = parse_fetch(sql)?;
            let mut out_cols = Vec::new();
            let mut out_rows = Vec::new();
            engine
                .txns()
                .with_mut(session, |t| {
                    let portal = t.portals.get_mut(&name).ok_or_else(|| {
                        spacestorage_query::ExecError::Msg(format!("unknown_portal:{name}"))
                    })?;
                    out_cols = portal.columns.clone();
                    for _ in 0..n {
                        if portal.pos >= portal.rows.len() {
                            break;
                        }
                        out_rows.push(portal.rows[portal.pos].clone());
                        portal.pos += 1;
                    }
                    Ok(())
                })
                .map_err(map_exec_err)?;
            Ok(ExecResult {
                tag: format!("FETCH {}", out_rows.len()),
                columns: out_cols,
                rows: out_rows,
            })
        }
        "CLOSE" => {
            let name = parse_close(sql);
            engine
                .txns()
                .with_mut(session, |t| {
                    t.portals.remove(&name);
                    Ok(())
                })
                .map_err(map_exec_err)?;
            Ok(ExecResult {
                tag: "CLOSE CURSOR".into(),
                columns: vec![],
                rows: vec![],
            })
        }
        _ => Err(ExecError {
            sqlstate: FEATURE_NOT_SUPPORTED.into(),
            message: format!("feature_not_supported: {verb}"),
        }),
    }
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
    execute_sql_cancel(
        profile,
        catalog,
        namespace,
        session,
        sql,
        CancelToken::new(),
        &Limits::default(),
    )
}

pub fn execute_sql_cancel(
    profile: DialectProfile,
    catalog: &SharedCatalog,
    namespace: &str,
    session: &str,
    sql: &str,
    cancel: CancelToken,
    limits: &Limits,
) -> Result<ExecResult, ExecError> {
    let sql = sql.trim().trim_end_matches(';');
    if sql.is_empty() {
        return Ok(ExecResult {
            tag: "EMPTY".into(),
            columns: vec![],
            rows: vec![],
        });
    }

    cancel.check().map_err(map_exec_err)?;

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
        return run_cursor(profile, catalog, namespace, session, sql, &verb, &cancel);
    }

    // Query text size gate (FR-011) — effective configured Limits, named limit_exceeded.
    if let Err(e) = limits.check_query_text(sql.len() as u64) {
        return Err(ExecError {
            sqlstate: "54000".into(),
            message: e.to_string(),
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
        cancel,
    ) {
        Ok(r) => {
            // Result size gate.
            let bytes: u64 = r
                .rows
                .iter()
                .flat_map(|row| row.iter())
                .map(|c| c.as_ref().map(|s| s.len() as u64).unwrap_or(0))
                .sum();
            if let Err(e) = limits.check_result(bytes) {
                return Err(ExecError {
                    sqlstate: "54000".into(),
                    message: e.to_string(),
                });
            }
            Ok(map_query_result(r))
        }
        Err(e) => Err(map_exec_err(e)),
    }
}

/// Drop all portals for a session (disconnect / force cancel).
pub fn drop_session_portals(session: &str) {
    let engine = PlannerEngine::with_distributed(
        Arc::new(RwLock::new(ContainerCatalog::new())),
        true,
    );
    let _ = engine.txns().with_mut(session, |t| {
        t.clear_portals();
        Ok(())
    });
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

    #[test]
    fn declare_fetch_close_in_txn() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let p = DialectProfile::CompleteProduct;
        let s = "cursor-t063";
        execute_sql_session(p, &cat, "ns", s, "BEGIN").unwrap();
        execute_sql_session(p, &cat, "ns", s, "CREATE TABLE t (id text, v text)").unwrap();
        execute_sql_session(p, &cat, "ns", s, "INSERT INTO t VALUES ('1', 'a')").unwrap();
        // COMMIT buffered insert so DECLARE SELECT sees rows (autocommit-style inside txn buffer).
        execute_sql_session(p, &cat, "ns", s, "COMMIT").unwrap();
        execute_sql_session(p, &cat, "ns", s, "BEGIN").unwrap();
        execute_sql_session(
            p,
            &cat,
            "ns",
            s,
            "DECLARE c CURSOR FOR SELECT * FROM t",
        )
        .unwrap();
        let fetched = execute_sql_session(p, &cat, "ns", s, "FETCH 1 FROM c").unwrap();
        assert_eq!(fetched.rows.len(), 1);
        execute_sql_session(p, &cat, "ns", s, "CLOSE c").unwrap();
        execute_sql_session(p, &cat, "ns", s, "COMMIT").unwrap();
    }

    #[test]
    fn with_hold_refused() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let p = DialectProfile::CompleteProduct;
        let s = "hold-t063";
        execute_sql_session(p, &cat, "ns", s, "BEGIN").unwrap();
        let e = execute_sql_session(
            p,
            &cat,
            "ns",
            s,
            "DECLARE c CURSOR WITH HOLD FOR SELECT 1",
        )
        .unwrap_err();
        assert_eq!(e.sqlstate, FEATURE_NOT_SUPPORTED);
        execute_sql_session(p, &cat, "ns", s, "ROLLBACK").unwrap();
    }

    #[test]
    fn snapshot_kv_refused() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        {
            use spacestorage_types::{L3Model, StorageModeChoice};
            let mut g = cat.write().unwrap();
            g.create("ns", "kv", L3Model::KvStore, false, None, StorageModeChoice::Memory)
                .unwrap();
        }
        let p = DialectProfile::CompleteProduct;
        let s = "snap-t062";
        execute_sql_session(p, &cat, "ns", s, "BEGIN ISOLATION LEVEL REPEATABLE READ")
            .unwrap();
        let e = execute_sql_session(p, &cat, "ns", s, "SELECT * FROM kv").unwrap_err();
        assert!(
            e.message.contains("snapshot_unsupported") || e.sqlstate == FEATURE_NOT_SUPPORTED,
            "{}",
            e.message
        );
        assert!(
            e.message.contains("K/V") || e.message.contains("READ COMMITTED"),
            "{}",
            e.message
        );
        execute_sql_session(p, &cat, "ns", s, "ROLLBACK").unwrap();
    }

    #[test]
    fn cancel_token_aborts() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let p = DialectProfile::FirstBinary;
        let tok = CancelToken::new();
        tok.cancel();
        let e =
            execute_sql_cancel(p, &cat, "ns", "c", "SELECT 1", tok, &Limits::default()).unwrap_err();
        assert_eq!(e.sqlstate, "57014");
    }
}

//! PostgreSQL SQL — classify via `015`, execute via shared `PlannerEngine` (005).

use spacestorage_compat::{ClassifyOutcome, DialectProfile, Limits, classify_pg_verb};
use spacestorage_query::{
    CancelToken, CursorPortal, PlannerEngine, QueryOptions, QueryResult, lower_sql,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NamespaceCommand {
    Create(String),
    Drop(String),
    Rename { name: String, new_name: String },
}

pub(crate) fn parse_namespace_command(sql: &str) -> Option<NamespaceCommand> {
    let parts: Vec<&str> = sql
        .trim()
        .trim_end_matches(';')
        .split_whitespace()
        .collect();
    match parts.as_slice() {
        [create, database, name]
            if create.eq_ignore_ascii_case("create")
                && database.eq_ignore_ascii_case("database") =>
        {
            Some(NamespaceCommand::Create((*name).trim_matches('"').into()))
        }
        [drop, database, name, ..]
            if drop.eq_ignore_ascii_case("drop")
                && database.eq_ignore_ascii_case("database") =>
        {
            Some(NamespaceCommand::Drop((*name).trim_matches('"').into()))
        }
        [alter, database, name, rename, to, new_name]
            if alter.eq_ignore_ascii_case("alter")
                && database.eq_ignore_ascii_case("database")
                && rename.eq_ignore_ascii_case("rename")
                && to.eq_ignore_ascii_case("to") =>
        {
            Some(NamespaceCommand::Rename {
                name: (*name).trim_matches('"').into(),
                new_name: (*new_name).trim_matches('"').into(),
            })
        }
        _ => None,
    }
}

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

fn cursor_declare_forbidden(upper: &str) -> Option<&'static str> {
    if upper.contains("WITH HOLD") {
        return Some("WITH HOLD");
    }
    if upper.contains("INSENSITIVE") {
        return Some("INSENSITIVE");
    }
    if scrollable_cursor_declared(upper) {
        return Some("SCROLL");
    }
    None
}

/// True for `SCROLL` / holdable scroll forms, not for the MUST `NO SCROLL` prefix.
fn scrollable_cursor_declared(upper: &str) -> bool {
    let Some(pos) = upper.find("SCROLL") else {
        return false;
    };
    let before = upper[..pos].trim_end();
    !before.ends_with("NO")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FetchLimit {
    Count(usize),
    All,
}

/// Parse `DECLARE name [NO SCROLL] CURSOR [WITHOUT HOLD] FOR <query>`.
fn parse_declare(sql: &str) -> Result<(String, String), ExecError> {
    let upper = sql.to_ascii_uppercase();
    if let Some(kind) = cursor_declare_forbidden(&upper) {
        return Err(ExecError {
            sqlstate: FEATURE_NOT_SUPPORTED.into(),
            message: format!("feature_not_supported: {kind}"),
        });
    }
    let parts: Vec<&str> = sql.split_whitespace().collect();
    // DECLARE <name> …
    let name = parts.get(1).copied().unwrap_or("c").to_string();
    let for_pos = upper.find(" FOR ").ok_or_else(|| ExecError {
        sqlstate: FEATURE_NOT_SUPPORTED.into(),
        message: "DECLARE requires FOR <query>".into(),
    })?;
    let query = sql[for_pos + 5..].trim().to_string();
    Ok((name, query))
}

fn parse_fetch(sql: &str) -> Result<(String, FetchLimit), ExecError> {
    // FETCH [FORWARD] [n|ALL] FROM name | FETCH name
    let upper = sql.to_ascii_uppercase();
    if upper.contains("BACKWARD") {
        return Err(ExecError {
            sqlstate: FEATURE_NOT_SUPPORTED.into(),
            message: "feature_not_supported: FETCH BACKWARD".into(),
        });
    }
    if upper.contains(" PRIOR") || parts_has_token(&upper, "PRIOR") {
        return Err(ExecError {
            sqlstate: FEATURE_NOT_SUPPORTED.into(),
            message: "feature_not_supported: FETCH PRIOR".into(),
        });
    }
    if upper.contains("ABSOLUTE") {
        return Err(ExecError {
            sqlstate: FEATURE_NOT_SUPPORTED.into(),
            message: "feature_not_supported: FETCH ABSOLUTE".into(),
        });
    }
    let parts: Vec<&str> = sql.split_whitespace().collect();
    let mut limit = FetchLimit::Count(1);
    let mut name = String::from("c");
    if let Some(from_i) = parts.iter().position(|p| p.eq_ignore_ascii_case("FROM")) {
        name = parts.get(from_i + 1).copied().unwrap_or("c").to_string();
        for p in &parts[1..from_i] {
            if p.eq_ignore_ascii_case("ALL") {
                limit = FetchLimit::All;
            } else if let Ok(v) = p.parse::<usize>() {
                limit = FetchLimit::Count(v);
            }
        }
    } else if parts.len() >= 2 {
        name = parts[1].to_string();
    }
    Ok((name, limit))
}

fn parts_has_token(upper: &str, token: &str) -> bool {
    upper
        .split_whitespace()
        .any(|p| p.eq_ignore_ascii_case(token))
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
    if let Some(kind) = cursor_declare_forbidden(&upper) {
        return Err(ExecError {
            sqlstate: FEATURE_NOT_SUPPORTED.into(),
            message: format!("feature_not_supported: {kind}"),
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
            let (name, limit) = parse_fetch(sql)?;
            let mut out_cols = Vec::new();
            let mut out_rows = Vec::new();
            engine
                .txns()
                .with_mut(session, |t| {
                    let portal = t.portals.get_mut(&name).ok_or_else(|| {
                        spacestorage_query::ExecError::Msg(format!("unknown_portal:{name}"))
                    })?;
                    out_cols = portal.columns.clone();
                    match limit {
                        FetchLimit::All => {
                            while portal.pos < portal.rows.len() {
                                out_rows.push(portal.rows[portal.pos].clone());
                                portal.pos += 1;
                            }
                        }
                        FetchLimit::Count(n) => {
                            for _ in 0..n {
                                if portal.pos >= portal.rows.len() {
                                    break;
                                }
                                out_rows.push(portal.rows[portal.pos].clone());
                                portal.pos += 1;
                            }
                        }
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
    execute_sql_cancel_tenancy(
        profile,
        catalog,
        namespace,
        session,
        sql,
        cancel,
        limits,
        None,
        None,
    )
}

pub fn execute_sql_cancel_tenancy(
    profile: DialectProfile,
    catalog: &SharedCatalog,
    namespace: &str,
    session: &str,
    sql: &str,
    cancel: CancelToken,
    limits: &Limits,
    tenancy: Option<&std::sync::Arc<spacestorage_tenancy::Tenancy>>,
    principal: Option<&str>,
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
    let mut engine = PlannerEngine::with_distributed(Arc::clone(catalog), distributed);
    if let Some(t) = tenancy {
        engine = engine.with_tenancy(std::sync::Arc::clone(t));
    }
    let req = lower_sql(profile, sql).map_err(|e| ExecError {
        sqlstate: FEATURE_NOT_SUPPORTED.into(),
        message: e.to_string(),
    })?;
    let mut options = QueryOptions::default();
    if let Some(login) = principal {
        options.principal = Some(login.to_string());
    }

    match engine.execute_blocking(
        namespace,
        session,
        "postgresql",
        req,
        options,
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
    let engine =
        PlannerEngine::with_distributed(Arc::new(RwLock::new(ContainerCatalog::new())), true);
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
        execute_sql_session(p, &cat, "ns", s, "DECLARE c CURSOR FOR SELECT * FROM t").unwrap();
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
        let e = execute_sql_session(p, &cat, "ns", s, "DECLARE c CURSOR WITH HOLD FOR SELECT 1")
            .unwrap_err();
        assert_eq!(e.sqlstate, FEATURE_NOT_SUPPORTED);
        execute_sql_session(p, &cat, "ns", s, "ROLLBACK").unwrap();
    }

    #[test]
    fn no_scroll_declare_and_fetch_count_all() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let p = DialectProfile::CompleteProduct;
        let s = "no-scroll-t085";
        execute_sql_session(p, &cat, "ns", s, "BEGIN").unwrap();
        execute_sql_session(p, &cat, "ns", s, "CREATE TABLE t (id text, v text)").unwrap();
        execute_sql_session(p, &cat, "ns", s, "INSERT INTO t VALUES ('1', 'a')").unwrap();
        execute_sql_session(p, &cat, "ns", s, "INSERT INTO t VALUES ('2', 'b')").unwrap();
        execute_sql_session(p, &cat, "ns", s, "INSERT INTO t VALUES ('3', 'c')").unwrap();
        execute_sql_session(p, &cat, "ns", s, "COMMIT").unwrap();
        execute_sql_session(p, &cat, "ns", s, "BEGIN").unwrap();
        execute_sql_session(
            p,
            &cat,
            "ns",
            s,
            "DECLARE c NO SCROLL CURSOR FOR SELECT * FROM t ORDER BY id",
        )
        .unwrap();
        let two = execute_sql_session(p, &cat, "ns", s, "FETCH FORWARD 2 FROM c").unwrap();
        assert_eq!(two.rows.len(), 2);
        let rest = execute_sql_session(p, &cat, "ns", s, "FETCH ALL FROM c").unwrap();
        assert_eq!(rest.rows.len(), 1);
        execute_sql_session(p, &cat, "ns", s, "CLOSE c").unwrap();
        execute_sql_session(p, &cat, "ns", s, "COMMIT").unwrap();
    }

    #[test]
    fn cursor_forbidden_variants_refused() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let p = DialectProfile::CompleteProduct;
        let s = "cursor-forbidden-t085";
        execute_sql_session(p, &cat, "ns", s, "BEGIN").unwrap();
        for sql in [
            "DECLARE c INSENSITIVE CURSOR FOR SELECT 1",
            "DECLARE c SCROLL CURSOR FOR SELECT 1",
        ] {
            let e = execute_sql_session(p, &cat, "ns", s, sql).unwrap_err();
            assert_eq!(e.sqlstate, FEATURE_NOT_SUPPORTED, "{sql}");
        }
        execute_sql_session(p, &cat, "ns", s, "DECLARE c NO SCROLL CURSOR FOR SELECT 1").unwrap();
        for sql in ["FETCH PRIOR FROM c", "FETCH ABSOLUTE 1 FROM c", "FETCH BACKWARD FROM c"] {
            let e = execute_sql_session(p, &cat, "ns", s, sql).unwrap_err();
            assert_eq!(e.sqlstate, FEATURE_NOT_SUPPORTED, "{sql}");
        }
        execute_sql_session(p, &cat, "ns", s, "ROLLBACK").unwrap();
    }

    #[test]
    fn snapshot_kv_refused() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        {
            use spacestorage_types::{L3Model, StorageModeChoice};
            let mut g = cat.write().unwrap();
            g.create(
                "ns",
                "kv",
                L3Model::KvStore,
                false,
                None,
                StorageModeChoice::Memory,
            )
            .unwrap();
        }
        let p = DialectProfile::CompleteProduct;
        let s = "snap-t062";
        execute_sql_session(p, &cat, "ns", s, "BEGIN ISOLATION LEVEL REPEATABLE READ").unwrap();
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
        let e = execute_sql_cancel(p, &cat, "ns", "c", "SELECT 1", tok, &Limits::default())
            .unwrap_err();
        assert_eq!(e.sqlstate, "57014");
    }

    #[test]
    fn parses_database_registry_operations() {
        assert_eq!(
            parse_namespace_command("CREATE DATABASE acme"),
            Some(NamespaceCommand::Create("acme".into()))
        );
        assert_eq!(
            parse_namespace_command("DROP DATABASE acme"),
            Some(NamespaceCommand::Drop("acme".into()))
        );
        assert_eq!(
            parse_namespace_command("ALTER DATABASE acme RENAME TO contoso"),
            Some(NamespaceCommand::Rename {
                name: "acme".into(),
                new_name: "contoso".into(),
            })
        );
    }
}

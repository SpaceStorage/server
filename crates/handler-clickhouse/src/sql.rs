//! Minimal SQL → dispatch mapping for ClickHouse smoke.

use crate::dispatch::{dispatch, ClickHouseReply, SessionState};

/// Map a SQL string to dispatch; returns None if empty.
pub fn execute_sql(session: &mut SessionState, sql: &str) -> ClickHouseReply {
    let sql = sql.trim().trim_end_matches(';');
    if sql.is_empty() {
        return ClickHouseReply::Ok;
    }
    if let Err(e) = session
        .limit_services
        .limits()
        .check_query_text(sql.len() as u64)
    {
        return ClickHouseReply::Error {
            code: "limit_exceeded".into(),
            message: e.to_string(),
        };
    }
    let upper = sql.to_ascii_uppercase();
    if upper.starts_with("USE ") {
        let db = sql
            .split_whitespace()
            .nth(1)
            .map(|s| {
                s.trim_matches(|c| c == '`' || c == '"')
                    .trim_end_matches(';')
                    .to_string()
            })
            .unwrap_or_default();
        if db.is_empty() {
            return ClickHouseReply::Error {
                code: "62".into(),
                message: "missing database name".into(),
            };
        }
        if session.tenancy.is_some() {
            if let Err(err) = session.try_use_registry_namespace(&db) {
                return ClickHouseReply::Error {
                    code: if matches!(
                        err,
                        spacestorage_tenancy::TenancyError::NamespaceNotFound { .. }
                    ) {
                        "81".into()
                    } else {
                        "241".into()
                    },
                    message: err.to_string(),
                };
            }
            return ClickHouseReply::Ok;
        }
        session.namespace = db;
        return ClickHouseReply::Ok;
    }
    if !upper.starts_with("CREATE DATABASE") && !upper.starts_with("RENAME DATABASE") {
        if let Some(reply) = session.refuse_data_without_registry_accept() {
            return reply;
        }
    }
    if upper.contains("DICTIONARY") {
        return dispatch(session, "DICTIONARY", &[]);
    }
    if upper.contains("MATERIALIZED VIEW") {
        return dispatch(session, "MATERIALIZED_VIEW", &[]);
    }
    if upper.contains("REPLICATED") {
        return dispatch(session, "REPLICATION", &[]);
    }
    if upper.contains("GROUP BY") {
        let parts: Vec<&str> = sql.split_whitespace().collect();
        let table = parts
            .iter()
            .position(|p| p.eq_ignore_ascii_case("FROM"))
            .and_then(|i| parts.get(i + 1))
            .map(|t| t.trim_matches(|c| c == '`' || c == ';'))
            .unwrap_or("t");
        let group_col = upper
            .find("GROUP BY")
            .map(|pos| {
                sql[pos + 8..]
                    .trim()
                    .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
                    .next()
                    .unwrap_or("id")
                    .trim_matches(|c| c == '`' || c == '"')
            })
            .unwrap_or("id");
        return dispatch(session, "GROUP_BY", &[table, group_col, group_col]);
    }
    if upper.starts_with("CREATE") {
        if upper.starts_with("CREATE DATABASE") {
            return ClickHouseReply::Error {
                code: "server_error".into(),
                message: "CREATE DATABASE requires namespace control path".into(),
            };
        }
        let parts: Vec<&str> = sql.split_whitespace().collect();
        // CREATE TABLE name …
        let table = parts
            .iter()
            .position(|p| p.eq_ignore_ascii_case("TABLE"))
            .and_then(|i| parts.get(i + 1))
            .map(|t| t.trim_matches(|c| c == '`' || c == '('))
            .unwrap_or("t");
        return dispatch(session, "CREATE", &["TABLE", table]);
    }
    if upper.starts_with("INSERT") {
        // INSERT INTO t VALUES (1,'x') or INSERT INTO t FORMAT … — smoke: INTO name id val
        let parts: Vec<&str> = sql.split_whitespace().collect();
        let table = parts
            .iter()
            .position(|p| p.eq_ignore_ascii_case("INTO"))
            .and_then(|i| parts.get(i + 1))
            .map(|t| t.trim_matches(|c| c == '`' || c == '('))
            .unwrap_or("t");
        // Try to pull simple values from VALUES (k, v)
        let (key, val) = parse_values_pair(sql).unwrap_or(("1".into(), "x".into()));
        return dispatch(session, "INSERT", &[table, &key, &val]);
    }
    if upper.starts_with("SELECT") {
        let parts: Vec<&str> = sql.split_whitespace().collect();
        let table = parts
            .iter()
            .position(|p| p.eq_ignore_ascii_case("FROM"))
            .and_then(|i| parts.get(i + 1))
            .map(|t| t.trim_matches(|c| c == '`' || c == ';'))
            .unwrap_or("t");
        if let Some(pos) = upper.find("WHERE") {
            let rest = sql[pos + 5..].trim();
            // id = '1' or id = 1
            let filter = rest
                .split('=')
                .nth(1)
                .map(|s| s.trim().trim_matches(|c| c == '\'' || c == '"' || c == ';'))
                .unwrap_or("");
            return dispatch(session, "SELECT_WHERE", &[table, filter]);
        }
        return dispatch(session, "SELECT", &[table]);
    }
    ClickHouseReply::Error {
        code: "48".into(),
        message: format!("NOT_IMPLEMENTED: {sql}"),
    }
}

fn parse_values_pair(sql: &str) -> Option<(String, String)> {
    let upper = sql.to_ascii_uppercase();
    let idx = upper.find("VALUES")?;
    let rest = sql[idx + 6..].trim();
    let start = rest.find('(')?;
    let end = rest.find(')')?;
    let inner = &rest[start + 1..end];
    let mut parts = inner.splitn(2, ',');
    let k = parts.next()?.trim().trim_matches(|c| c == '\'' || c == '"');
    let v = parts.next()?.trim().trim_matches(|c| c == '\'' || c == '"');
    Some((k.to_string(), v.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use spacestorage_types::ContainerCatalog;
    use std::sync::{Arc, RwLock};

    #[test]
    fn use_accepts_registered_database() {
        use spacestorage_authz::Verb;
        use spacestorage_tenancy::Tenancy;
        use std::sync::Arc;

        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let tenancy = Arc::new(Tenancy::new());
        tenancy
            .registry
            .lock()
            .create(Verb::CLUSTER_ADMIN.bits(), "acme")
            .unwrap();
        let mut s = SessionState {
            catalog: Arc::clone(&cat),
            namespace: "demo".into(),
            profile: spacestorage_compat::DialectProfile::HandlersComplete,
            protocol: spacestorage_compat::ProtocolId::ClickHouse,
            limit_services: spacestorage_compat::LimitServices::built_in(),
            principal_hold: None,
            tenancy: Some(tenancy),
            namespace_control: None,
            connection_quota: None,
            connection_charged_namespace: None,
            principal: Some("demo".into()),
            registry_accepted: false,
        };
        match execute_sql(&mut s, "SELECT 1") {
            ClickHouseReply::Error { message, .. } => {
                assert!(message.contains("NamespaceNotFound"), "{message}");
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(execute_sql(&mut s, "USE acme"), ClickHouseReply::Ok));
        assert!(s.registry_accepted);
        assert!(matches!(
            execute_sql(&mut s, "CREATE TABLE t (id String)"),
            ClickHouseReply::Ok
        ));
    }

    #[test]
    fn create_database_not_treated_as_create_table() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo_native(cat);
        match execute_sql(&mut s, "CREATE DATABASE acme") {
            ClickHouseReply::Error { message, .. } => {
                assert!(message.contains("namespace control"), "{message}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn select_allowed_after_registry_accept() {
        use spacestorage_authz::Verb;
        use spacestorage_tenancy::Tenancy;
        use std::sync::Arc;

        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let tenancy = Arc::new(Tenancy::new());
        tenancy
            .registry
            .lock()
            .create(Verb::CLUSTER_ADMIN.bits(), "demo")
            .unwrap();
        let mut s = SessionState {
            catalog: Arc::clone(&cat),
            namespace: "demo".into(),
            profile: spacestorage_compat::DialectProfile::HandlersComplete,
            protocol: spacestorage_compat::ProtocolId::ClickHouse,
            limit_services: spacestorage_compat::LimitServices::built_in(),
            principal_hold: None,
            tenancy: Some(tenancy),
            namespace_control: None,
            connection_quota: None,
            connection_charged_namespace: None,
            principal: Some("demo".into()),
            registry_accepted: true,
        };
        assert!(matches!(
            execute_sql(&mut s, "CREATE TABLE t (id String, v String)"),
            ClickHouseReply::Ok
        ));
    }

    #[test]
    fn materialized_view_and_replicated_must_not() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo_native(cat);
        match execute_sql(
            &mut s,
            "CREATE MATERIALIZED VIEW mv ENGINE = MergeTree() AS SELECT 1",
        ) {
            ClickHouseReply::Error { message, .. } => {
                assert!(
                    message.contains("compat_must_not") || message.contains("CompatMustNot"),
                    "{message}"
                );
            }
            other => panic!("{other:?}"),
        }
        match execute_sql(
            &mut s,
            "CREATE TABLE r (id String) ENGINE = ReplicatedMergeTree('/clickhouse/tables/r', '1') ORDER BY id",
        ) {
            ClickHouseReply::Error { message, .. } => {
                assert!(
                    message.contains("compat_must_not") || message.contains("CompatMustNot"),
                    "{message}"
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn create_insert_select() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo_native(cat);
        assert!(matches!(
            execute_sql(&mut s, "CREATE TABLE t (id String, v String)"),
            ClickHouseReply::Ok
        ));
        assert!(matches!(
            execute_sql(&mut s, "INSERT INTO t VALUES ('1','x')"),
            ClickHouseReply::Ok
        ));
        match execute_sql(&mut s, "SELECT * FROM t WHERE id = '1'") {
            ClickHouseReply::Rows(r) => assert_eq!(r.len(), 1),
            other => panic!("{other:?}"),
        }
    }
}

//! Minimal SQL → dispatch mapping for ClickHouse smoke.

use crate::dispatch::{dispatch, ClickHouseReply, SessionState};

/// Map a SQL string to dispatch; returns None if empty.
pub fn execute_sql(session: &mut SessionState, sql: &str) -> ClickHouseReply {
    let sql = sql.trim().trim_end_matches(';');
    if sql.is_empty() {
        return ClickHouseReply::Ok;
    }
    let upper = sql.to_ascii_uppercase();
    if upper.contains("DICTIONARY") {
        return dispatch(session, "DICTIONARY", &[]);
    }
    if upper.contains("GROUP BY") {
        return dispatch(session, "GROUP_BY", &[]);
    }
    if upper.starts_with("CREATE") {
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

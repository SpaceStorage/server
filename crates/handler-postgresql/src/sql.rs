//! First-binary SQL lowering — classify via `015` before catalog IR.

use serde_json::{Map, Value};
use spacestorage_compat::{classify_pg_verb, ClassifyOutcome, DialectProfile};
use spacestorage_types::{
    ContainerCatalog, ContainerSchema, Field, L3Model, StorageModeChoice, ValueDomain,
};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

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

/// Execute one auto-commit SQL statement against the catalog.
pub fn execute_sql(
    profile: DialectProfile,
    catalog: &SharedCatalog,
    namespace: &str,
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
    let outcome = classify_pg_verb(profile, &verb);
    if !outcome.is_must() {
        return Err(refuse(outcome, &verb));
    }

    match verb.as_str() {
        "BEGIN" | "COMMIT" | "ROLLBACK" | "START" | "COPY" => {
            // classify should have refused; belt-and-suspenders
            Err(ExecError {
                sqlstate: FEATURE_NOT_SUPPORTED.into(),
                message: format!("feature_not_supported: {verb}"),
            })
        }
        "CREATE" => create_table(catalog, namespace, sql),
        "DROP" => drop_table(catalog, namespace, sql),
        "INSERT" => insert_row(catalog, namespace, sql),
        "SELECT" => select_rows(catalog, namespace, sql),
        "UPDATE" => update_rows(catalog, namespace, sql),
        "DELETE" => delete_rows(catalog, namespace, sql),
        "SET" | "SHOW" | "DISCARD" | "RESET" => Ok(ExecResult {
            tag: format!("{verb}"),
            columns: vec![],
            rows: vec![],
        }),
        "PREPARE" | "EXECUTE" | "DEALLOCATE" | "EXPLAIN" => Ok(ExecResult {
            tag: format!("{verb}"),
            columns: vec![],
            rows: vec![],
        }),
        other => Err(ExecError {
            sqlstate: FEATURE_NOT_SUPPORTED.into(),
            message: format!("feature_not_supported: {other}"),
        }),
    }
}

fn create_table(catalog: &SharedCatalog, namespace: &str, sql: &str) -> Result<ExecResult, ExecError> {
    // CREATE TABLE [IF NOT EXISTS] name (col type, ...)
    let upper = sql.to_ascii_uppercase();
    if !upper.contains("TABLE") {
        return Err(ExecError {
            sqlstate: FEATURE_NOT_SUPPORTED.into(),
            message: "feature_not_supported: CREATE without TABLE".into(),
        });
    }
    let name = extract_table_name_after(&upper, sql, "TABLE")?;
    let if_not_exists = upper.contains("IF NOT EXISTS");
    let cols = parse_column_list(sql);

    let schema = ContainerSchema {
        fields: cols
            .iter()
            .map(|c| Field {
                name: c.clone(),
                domain: ValueDomain::Utf8,
                nullable: true,
            })
            .collect(),
    };
    if schema.fields.is_empty() {
        return Err(ExecError {
            sqlstate: "42601".into(),
            message: "syntax error: CREATE TABLE requires columns".into(),
        });
    }

    let mut cat = catalog.write().expect("catalog");
    match cat.create(
        namespace,
        &name,
        L3Model::RelationalTable,
        false,
        Some(schema),
        StorageModeChoice::Memory,
    ) {
        Ok(_) => Ok(ExecResult {
            tag: "CREATE TABLE".into(),
            columns: vec![],
            rows: vec![],
        }),
        Err(spacestorage_types::TypeError::AlreadyExists) if if_not_exists => Ok(ExecResult {
            tag: "CREATE TABLE".into(),
            columns: vec![],
            rows: vec![],
        }),
        Err(e) => Err(ExecError {
            sqlstate: "42P07".into(),
            message: e.to_string(),
        }),
    }
}

fn drop_table(catalog: &SharedCatalog, namespace: &str, sql: &str) -> Result<ExecResult, ExecError> {
    let upper = sql.to_ascii_uppercase();
    if !upper.contains("TABLE") {
        return Err(ExecError {
            sqlstate: FEATURE_NOT_SUPPORTED.into(),
            message: "feature_not_supported: DROP without TABLE".into(),
        });
    }
    let name = extract_table_name_after(&upper, sql, "TABLE")?;
    let if_exists = upper.contains("IF EXISTS");
    let mut cat = catalog.write().expect("catalog");
    match cat.drop_container(namespace, &name) {
        Ok(()) => Ok(ExecResult {
            tag: "DROP TABLE".into(),
            columns: vec![],
            rows: vec![],
        }),
        Err(spacestorage_types::TypeError::NotFound) if if_exists => Ok(ExecResult {
            tag: "DROP TABLE".into(),
            columns: vec![],
            rows: vec![],
        }),
        Err(e) => Err(ExecError {
            sqlstate: "42P01".into(),
            message: e.to_string(),
        }),
    }
}

fn insert_row(catalog: &SharedCatalog, namespace: &str, sql: &str) -> Result<ExecResult, ExecError> {
    // INSERT INTO name [(cols)] VALUES (v1, v2, ...)
    let upper = sql.to_ascii_uppercase();
    let name = extract_table_name_after(&upper, sql, "INTO")?;
    let values = parse_values_clause(sql)?;
    let mut cat = catalog.write().expect("catalog");
    let c = cat.describe(namespace, &name).map_err(|e| ExecError {
        sqlstate: "42P01".into(),
        message: e.to_string(),
    })?;
    let id = c.id;
    let fields: Vec<String> = c
        .schema
        .as_ref()
        .map(|s| s.fields.iter().map(|f| f.name.clone()).collect())
        .unwrap_or_default();
    if fields.len() != values.len() && !fields.is_empty() {
        // allow positional fill of declared columns
        if values.len() > fields.len() {
            return Err(ExecError {
                sqlstate: "42601".into(),
                message: "INSERT column/value count mismatch".into(),
            });
        }
    }
    let mut map = Map::new();
    for (i, v) in values.iter().enumerate() {
        let col = fields.get(i).cloned().unwrap_or_else(|| format!("c{i}"));
        map.insert(col, Value::String(v.clone()));
    }
    let pk = fields
        .first()
        .and_then(|f| map.get(f))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let bytes = serde_json::to_vec(&Value::Object(map)).map_err(|e| ExecError {
        sqlstate: "XX000".into(),
        message: e.to_string(),
    })?;
    cat.put(id, &pk, bytes).map_err(|e| ExecError {
        sqlstate: "XX000".into(),
        message: e.to_string(),
    })?;
    Ok(ExecResult {
        tag: "INSERT 0 1".into(),
        columns: vec![],
        rows: vec![],
    })
}

fn select_rows(catalog: &SharedCatalog, namespace: &str, sql: &str) -> Result<ExecResult, ExecError> {
    let upper = sql.to_ascii_uppercase();
    if upper.contains(" FROM ") {
        let name = extract_table_name_after(&upper, sql, "FROM")?;
        let cat = catalog.read().expect("catalog");
        let c = cat.describe(namespace, &name).map_err(|e| ExecError {
            sqlstate: "42P01".into(),
            message: e.to_string(),
        })?;
        let id = c.id;
        let cols: Vec<String> = c
            .schema
            .as_ref()
            .map(|s| s.fields.iter().map(|f| f.name.clone()).collect())
            .unwrap_or_else(|| vec!["value".into()]);
        let keys = cat.keys(id).map_err(|e| ExecError {
            sqlstate: "XX000".into(),
            message: e.to_string(),
        })?;
        let where_eq = parse_simple_where(sql);
        let mut rows = Vec::new();
        for k in keys {
            let raw = cat.get(id, &k).map_err(|e| ExecError {
                sqlstate: "XX000".into(),
                message: e.to_string(),
            })?;
            let Some(raw) = raw else { continue };
            let obj: Value = serde_json::from_slice(raw).unwrap_or(Value::Null);
            if let Some((wk, wv)) = &where_eq {
                let ok = obj
                    .get(wk)
                    .and_then(|v| v.as_str())
                    .map(|s| s == wv)
                    .unwrap_or(false);
                if !ok {
                    continue;
                }
            }
            let mut row = Vec::new();
            for col in &cols {
                let cell = obj
                    .get(col)
                    .and_then(|v| match v {
                        Value::String(s) => Some(s.clone()),
                        Value::Null => None,
                        other => Some(other.to_string()),
                    });
                row.push(cell);
            }
            rows.push(row);
        }
        Ok(ExecResult {
            tag: format!("SELECT {}", rows.len()),
            columns: cols,
            rows,
        })
    } else {
        // SELECT 1 / SELECT constants
        Ok(ExecResult {
            tag: "SELECT 1".into(),
            columns: vec!["?column?".into()],
            rows: vec![vec![Some("1".into())]],
        })
    }
}

fn update_rows(catalog: &SharedCatalog, namespace: &str, sql: &str) -> Result<ExecResult, ExecError> {
    // UPDATE name SET col = 'v' [WHERE col = 'x']
    let name = extract_ident_after_verb(sql, "UPDATE")?;
    let set = parse_set_clause(sql)?;
    let where_eq = parse_simple_where(sql);
    let mut cat = catalog.write().expect("catalog");
    let c = cat.describe(namespace, &name).map_err(|e| ExecError {
        sqlstate: "42P01".into(),
        message: e.to_string(),
    })?;
    let id = c.id;
    let keys = cat.keys(id).map_err(|e| ExecError {
        sqlstate: "XX000".into(),
        message: e.to_string(),
    })?;
    let mut n = 0i64;
    for k in keys {
        let raw = match cat.get(id, &k).ok().flatten() {
            Some(r) => r.to_vec(),
            None => continue,
        };
        let mut obj: Value = serde_json::from_slice(&raw).unwrap_or(Value::Object(Map::new()));
        if let Some((wk, wv)) = &where_eq {
            let ok = obj
                .get(wk)
                .and_then(|v| v.as_str())
                .map(|s| s == wv)
                .unwrap_or(false);
            if !ok {
                continue;
            }
        }
        if let Value::Object(map) = &mut obj {
            for (ck, cv) in &set {
                map.insert(ck.clone(), Value::String(cv.clone()));
            }
        }
        let bytes = serde_json::to_vec(&obj).map_err(|e| ExecError {
            sqlstate: "XX000".into(),
            message: e.to_string(),
        })?;
        cat.put(id, &k, bytes).map_err(|e| ExecError {
            sqlstate: "XX000".into(),
            message: e.to_string(),
        })?;
        n += 1;
    }
    Ok(ExecResult {
        tag: format!("UPDATE {n}"),
        columns: vec![],
        rows: vec![],
    })
}

fn delete_rows(catalog: &SharedCatalog, namespace: &str, sql: &str) -> Result<ExecResult, ExecError> {
    let upper = sql.to_ascii_uppercase();
    let name = extract_table_name_after(&upper, sql, "FROM")?;
    let where_eq = parse_simple_where(sql);
    let mut cat = catalog.write().expect("catalog");
    let c = cat.describe(namespace, &name).map_err(|e| ExecError {
        sqlstate: "42P01".into(),
        message: e.to_string(),
    })?;
    let id = c.id;
    let keys = cat.keys(id).map_err(|e| ExecError {
        sqlstate: "XX000".into(),
        message: e.to_string(),
    })?;
    let mut n = 0i64;
    for k in keys {
        if let Some((wk, wv)) = &where_eq {
            let raw = cat.get(id, &k).ok().flatten();
            let Some(raw) = raw else { continue };
            let obj: Value = serde_json::from_slice(raw).unwrap_or(Value::Null);
            let ok = obj
                .get(wk)
                .and_then(|v| v.as_str())
                .map(|s| s == wv)
                .unwrap_or(false);
            if !ok {
                continue;
            }
        }
        if cat.delete_row(id, &k).unwrap_or(false) {
            n += 1;
        }
    }
    Ok(ExecResult {
        tag: format!("DELETE {n}"),
        columns: vec![],
        rows: vec![],
    })
}

fn extract_table_name_after(upper: &str, original: &str, keyword: &str) -> Result<String, ExecError> {
    let kw = keyword.to_ascii_uppercase();
    let pos = upper
        .find(&kw)
        .ok_or_else(|| ExecError {
            sqlstate: "42601".into(),
            message: format!("syntax: missing {keyword}"),
        })?;
    let after = original[pos + keyword.len()..].trim_start();
    let after = after
        .trim_start_matches(|c: char| c.is_ascii_whitespace());
    // skip IF NOT EXISTS / IF EXISTS
    let mut rest = after;
    let ru = rest.to_ascii_uppercase();
    if ru.starts_with("IF NOT EXISTS") {
        rest = rest["IF NOT EXISTS".len()..].trim_start();
    } else if ru.starts_with("IF EXISTS") {
        rest = rest["IF EXISTS".len()..].trim_start();
    }
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '"')
        .collect::<String>()
        .trim_matches('"')
        .to_string();
    if name.is_empty() {
        return Err(ExecError {
            sqlstate: "42601".into(),
            message: "syntax: empty table name".into(),
        });
    }
    Ok(name)
}

fn extract_ident_after_verb(sql: &str, verb: &str) -> Result<String, ExecError> {
    let upper = sql.to_ascii_uppercase();
    let v = verb.to_ascii_uppercase();
    if !upper.starts_with(&v) {
        return Err(ExecError {
            sqlstate: "42601".into(),
            message: format!("expected {verb}"),
        });
    }
    let rest = sql[verb.len()..].trim_start();
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '"')
        .collect::<String>()
        .trim_matches('"')
        .to_string();
    if name.is_empty() {
        return Err(ExecError {
            sqlstate: "42601".into(),
            message: "empty relation name".into(),
        });
    }
    Ok(name)
}

fn parse_column_list(sql: &str) -> Vec<String> {
    let Some(start) = sql.find('(') else {
        return vec![];
    };
    let Some(end) = sql.rfind(')') else {
        return vec![];
    };
    if end <= start {
        return vec![];
    }
    sql[start + 1..end]
        .split(',')
        .filter_map(|part| {
            let tok = part.trim().split_whitespace().next()?;
            Some(tok.trim_matches('"').to_string())
        })
        .collect()
}

fn parse_values_clause(sql: &str) -> Result<Vec<String>, ExecError> {
    let upper = sql.to_ascii_uppercase();
    let pos = upper.find("VALUES").ok_or_else(|| ExecError {
        sqlstate: "42601".into(),
        message: "INSERT requires VALUES".into(),
    })?;
    let after = &sql[pos + 6..];
    let start = after
        .find('(')
        .ok_or_else(|| ExecError {
            sqlstate: "42601".into(),
            message: "VALUES needs (...)".into(),
        })?;
    let end = after[start..]
        .find(')')
        .ok_or_else(|| ExecError {
            sqlstate: "42601".into(),
            message: "VALUES needs closing )".into(),
        })?;
    let inner = &after[start + 1..start + end];
    Ok(split_sql_values(inner))
}

fn split_sql_values(inner: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    for ch in inner.chars() {
        match ch {
            '\'' if !in_quote => in_quote = true,
            '\'' if in_quote => in_quote = false,
            ',' if !in_quote => {
                out.push(unquote(cur.trim()));
                cur.clear();
            }
            c => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(unquote(cur.trim()));
    }
    out
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2 {
        s[1..s.len() - 1].replace("''", "'")
    } else {
        s.to_string()
    }
}

fn parse_simple_where(sql: &str) -> Option<(String, String)> {
    let upper = sql.to_ascii_uppercase();
    let pos = upper.find(" WHERE ")?;
    let rest = sql[pos + 7..].trim();
    let eq = rest.find('=')?;
    let col = rest[..eq].trim().trim_matches('"').to_string();
    let val = unquote(rest[eq + 1..].trim().trim_end_matches(';'));
    Some((col, val))
}

fn parse_set_clause(sql: &str) -> Result<Vec<(String, String)>, ExecError> {
    let upper = sql.to_ascii_uppercase();
    let pos = upper.find(" SET ").ok_or_else(|| ExecError {
        sqlstate: "42601".into(),
        message: "UPDATE requires SET".into(),
    })?;
    let rest = &sql[pos + 5..];
    let end = upper[pos + 5..]
        .find(" WHERE ")
        .map(|i| i)
        .unwrap_or(rest.len());
    let clause = rest[..end].trim();
    let mut out = Vec::new();
    for part in clause.split(',') {
        let eq = part.find('=').ok_or_else(|| ExecError {
            sqlstate: "42601".into(),
            message: "bad SET assignment".into(),
        })?;
        let col = part[..eq].trim().trim_matches('"').to_string();
        let val = unquote(part[eq + 1..].trim());
        out.push((col, val));
    }
    Ok(out)
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
}

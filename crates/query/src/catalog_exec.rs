//! Catalog-backed auto-commit SQL / row helpers used by PlannerEngine.

use crate::error::ExecError;
use crate::request::CopyFormat;
#[cfg(feature = "query-distributed")]
use crate::request::{AggFn, JoinKind};
use serde_json::{Map, Value};
use spacestorage_types::{
    ContainerCatalog, ContainerSchema, Field, L3Model, StorageModeChoice, ValueDomain,
};
use std::sync::{Arc, RwLock};

pub type SharedCatalog = Arc<RwLock<ContainerCatalog>>;

#[derive(Debug, Clone, Default)]
pub struct QueryResult {
    pub tag: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
}

pub fn execute_adhoc_sql(
    catalog: &SharedCatalog,
    namespace: &str,
    sql: &str,
) -> Result<QueryResult, ExecError> {
    let sql = sql.trim().trim_end_matches(';');
    if sql.is_empty() {
        return Ok(QueryResult {
            tag: "EMPTY".into(),
            ..Default::default()
        });
    }
    let verb = first_verb(sql);
    match verb.as_str() {
        "CREATE" => create_table(catalog, namespace, sql),
        "DROP" => drop_table(catalog, namespace, sql),
        "INSERT" => insert_row(catalog, namespace, sql),
        "SELECT" => select_rows(catalog, namespace, sql),
        "UPDATE" => update_rows(catalog, namespace, sql),
        "DELETE" => delete_rows(catalog, namespace, sql),
        "SET" | "SHOW" | "DISCARD" | "RESET" | "PREPARE" | "EXECUTE" | "DEALLOCATE" => {
            Ok(QueryResult {
                tag: verb,
                ..Default::default()
            })
        }
        other => Err(ExecError::NotSupported(other.into())),
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

fn create_table(catalog: &SharedCatalog, namespace: &str, sql: &str) -> Result<QueryResult, ExecError> {
    let upper = sql.to_ascii_uppercase();
    if !upper.contains("TABLE") {
        return Err(ExecError::NotSupported("CREATE without TABLE".into()));
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
        return Err(ExecError::Msg("syntax error: CREATE TABLE requires columns".into()));
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
        Ok(_) => Ok(QueryResult {
            tag: "CREATE TABLE".into(),
            ..Default::default()
        }),
        Err(spacestorage_types::TypeError::AlreadyExists) if if_not_exists => Ok(QueryResult {
            tag: "CREATE TABLE".into(),
            ..Default::default()
        }),
        Err(e) => Err(ExecError::Msg(e.to_string())),
    }
}

fn drop_table(catalog: &SharedCatalog, namespace: &str, sql: &str) -> Result<QueryResult, ExecError> {
    let upper = sql.to_ascii_uppercase();
    let name = extract_table_name_after(&upper, sql, "TABLE")?;
    let if_exists = upper.contains("IF EXISTS");
    let mut cat = catalog.write().expect("catalog");
    match cat.drop_container(namespace, &name) {
        Ok(()) => Ok(QueryResult {
            tag: "DROP TABLE".into(),
            ..Default::default()
        }),
        Err(spacestorage_types::TypeError::NotFound) if if_exists => Ok(QueryResult {
            tag: "DROP TABLE".into(),
            ..Default::default()
        }),
        Err(e) => Err(ExecError::Msg(e.to_string())),
    }
}

fn insert_row(catalog: &SharedCatalog, namespace: &str, sql: &str) -> Result<QueryResult, ExecError> {
    let upper = sql.to_ascii_uppercase();
    let name = extract_table_name_after(&upper, sql, "INTO")?;
    let values = parse_values(sql)?;
    let mut cat = catalog.write().expect("catalog");
    let c = cat
        .describe(namespace, &name)
        .map_err(|e| ExecError::Msg(e.to_string()))?;
    let id = c.id;
    let cols: Vec<String> = c
        .schema
        .as_ref()
        .map(|s| s.fields.iter().map(|f| f.name.clone()).collect())
        .unwrap_or_default();
    let mut map = Map::new();
    for (i, v) in values.iter().enumerate() {
        if let Some(col) = cols.get(i) {
            map.insert(col.clone(), Value::String(v.clone()));
        }
    }
    let key = values.first().cloned().unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
    let bytes = serde_json::to_vec(&Value::Object(map)).map_err(|e| ExecError::Msg(e.to_string()))?;
    cat.put(id, &key, bytes)
        .map_err(|e| ExecError::Msg(e.to_string()))?;
    Ok(QueryResult {
        tag: "INSERT 0 1".into(),
        ..Default::default()
    })
}

fn select_rows(catalog: &SharedCatalog, namespace: &str, sql: &str) -> Result<QueryResult, ExecError> {
    let upper = sql.to_ascii_uppercase();
    // JOIN / aggregate require query-distributed (slice 8)
    if upper.contains(" JOIN ") {
        #[cfg(feature = "query-distributed")]
        {
            return join_select(catalog, namespace, sql);
        }
        #[cfg(not(feature = "query-distributed"))]
        {
            return Err(ExecError::NotSupported("join requires query-distributed".into()));
        }
    }
    if let Some(agg) = detect_agg(&upper) {
        #[cfg(feature = "query-distributed")]
        {
            return aggregate_select(catalog, namespace, sql, agg);
        }
        #[cfg(not(feature = "query-distributed"))]
        {
            let _ = agg;
            return Err(ExecError::NotSupported(
                "aggregate requires query-distributed".into(),
            ));
        }
    }
    if upper.contains(" FROM ") {
        let name = extract_table_name_after(&upper, sql, "FROM")?;
        let cat = catalog.read().expect("catalog");
        let c = cat
            .describe(namespace, &name)
            .map_err(|e| ExecError::Msg(e.to_string()))?;
        let id = c.id;
        let cols: Vec<String> = c
            .schema
            .as_ref()
            .map(|s| s.fields.iter().map(|f| f.name.clone()).collect())
            .unwrap_or_else(|| vec!["value".into()]);
        let keys = cat.keys(id).map_err(|e| ExecError::Msg(e.to_string()))?;
        let where_eq = parse_simple_where(sql);
        let mut rows = Vec::new();
        for k in keys {
            let raw = cat.get(id, &k).map_err(|e| ExecError::Msg(e.to_string()))?;
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
                row.push(cell(&obj, col));
            }
            rows.push(row);
        }
        Ok(QueryResult {
            tag: format!("SELECT {}", rows.len()),
            columns: cols,
            rows,
        })
    } else {
        Ok(QueryResult {
            tag: "SELECT 1".into(),
            columns: vec!["?column?".into()],
            rows: vec![vec![Some("1".into())]],
        })
    }
}

fn detect_agg(upper: &str) -> Option<&'static str> {
    if upper.contains("COUNT(") {
        Some("count")
    } else if upper.contains("SUM(") {
        Some("sum")
    } else if upper.contains("MIN(") {
        Some("min")
    } else if upper.contains("MAX(") {
        Some("max")
    } else if upper.contains("AVG(") {
        Some("avg")
    } else {
        None
    }
}

#[cfg(feature = "query-distributed")]
fn aggregate_select(
    catalog: &SharedCatalog,
    namespace: &str,
    sql: &str,
    func_name: &str,
) -> Result<QueryResult, ExecError> {
    let func = match func_name {
        "count" => AggFn::Count,
        "sum" => AggFn::Sum,
        "min" => AggFn::Min,
        "max" => AggFn::Max,
        "avg" => AggFn::Avg,
        _ => return Err(ExecError::NotSupported(func_name.into())),
    };
    let upper = sql.to_ascii_uppercase();
    let name = extract_table_name_after(&upper, sql, "FROM")?;
    let group_by = if upper.contains(" GROUP BY ") {
        Some(
            sql[upper.find(" GROUP BY ").unwrap() + " GROUP BY ".len()..]
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_matches(|c: char| c == ',' || c == ';')
                .to_string(),
        )
    } else {
        None
    };
    crate::engines::aggregate::run(catalog, namespace, &name, func, group_by.as_deref(), None)
}

#[cfg(feature = "query-distributed")]
fn join_select(catalog: &SharedCatalog, namespace: &str, sql: &str) -> Result<QueryResult, ExecError> {
    let upper = sql.to_ascii_uppercase();
    let kind = if upper.contains(" LEFT JOIN ") {
        JoinKind::Left
    } else {
        JoinKind::Inner
    };
    let from_pos = upper
        .find(" FROM ")
        .ok_or_else(|| ExecError::Msg("join: missing FROM".into()))?;
    let after = &sql[from_pos + 6..];
    let left = after
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(|c: char| c == ',' || c == ';')
        .to_string();
    let join_kw = if kind == JoinKind::Left {
        " LEFT JOIN "
    } else {
        " JOIN "
    };
    let jpos = upper
        .find(join_kw)
        .ok_or_else(|| ExecError::Msg("join: missing JOIN".into()))?;
    let after_j = &sql[jpos + join_kw.len()..];
    let right = after_j
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string();
    let on_pos = upper
        .find(" ON ")
        .ok_or_else(|| ExecError::Msg("join: missing ON".into()))?;
    let on_clause = &sql[on_pos + 4..];
    let parts: Vec<&str> = on_clause.split('=').collect();
    if parts.len() != 2 {
        return Err(ExecError::Msg("join: expected equality ON".into()));
    }
    let left_key = parts[0]
        .trim()
        .rsplit('.')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    let right_key = parts[1]
        .split_whitespace()
        .next()
        .unwrap_or("")
        .rsplit('.')
        .next()
        .unwrap_or("")
        .trim_matches(';')
        .to_string();
    crate::engines::join::run(
        catalog,
        namespace,
        &left,
        &right,
        kind,
        &left_key,
        &right_key,
    )
}

fn update_rows(catalog: &SharedCatalog, namespace: &str, sql: &str) -> Result<QueryResult, ExecError> {
    let name = extract_ident_after_verb(sql, "UPDATE")?;
    let set = parse_set_clause(sql)?;
    let where_eq = parse_simple_where(sql);
    let mut cat = catalog.write().expect("catalog");
    let c = cat
        .describe(namespace, &name)
        .map_err(|e| ExecError::Msg(e.to_string()))?;
    let id = c.id;
    let keys = cat.keys(id).map_err(|e| ExecError::Msg(e.to_string()))?;
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
        let bytes = serde_json::to_vec(&obj).map_err(|e| ExecError::Msg(e.to_string()))?;
        cat.put(id, &k, bytes)
            .map_err(|e| ExecError::Msg(e.to_string()))?;
        n += 1;
    }
    Ok(QueryResult {
        tag: format!("UPDATE {n}"),
        ..Default::default()
    })
}

fn delete_rows(catalog: &SharedCatalog, namespace: &str, sql: &str) -> Result<QueryResult, ExecError> {
    let upper = sql.to_ascii_uppercase();
    let name = extract_table_name_after(&upper, sql, "FROM")?;
    let where_eq = parse_simple_where(sql);
    let mut cat = catalog.write().expect("catalog");
    let c = cat
        .describe(namespace, &name)
        .map_err(|e| ExecError::Msg(e.to_string()))?;
    let id = c.id;
    let keys = cat.keys(id).map_err(|e| ExecError::Msg(e.to_string()))?;
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
    Ok(QueryResult {
        tag: format!("DELETE {n}"),
        ..Default::default()
    })
}

pub fn copy_in(
    catalog: &SharedCatalog,
    namespace: &str,
    container: &str,
    format: CopyFormat,
    rows: &[Vec<Option<String>>],
) -> Result<QueryResult, ExecError> {
    let _ = format;
    let mut cat = catalog.write().expect("catalog");
    let c = cat
        .describe(namespace, container)
        .map_err(|e| ExecError::Msg(e.to_string()))?;
    let id = c.id;
    let cols: Vec<String> = c
        .schema
        .as_ref()
        .map(|s| s.fields.iter().map(|f| f.name.clone()).collect())
        .unwrap_or_default();
    if cols.is_empty() {
        return Err(ExecError::UnsupportedByType {
            container: container.into(),
            ty: "unknown".into(),
            op: "copy".into(),
        });
    }
    let mut n = 0i64;
    for row in rows {
        let mut map = Map::new();
        for (i, col) in cols.iter().enumerate() {
            if let Some(Some(v)) = row.get(i) {
                map.insert(col.clone(), Value::String(v.clone()));
            }
        }
        let key = row
            .first()
            .and_then(|c| c.clone())
            .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
        let bytes = serde_json::to_vec(&Value::Object(map)).map_err(|e| ExecError::Msg(e.to_string()))?;
        cat.put(id, &key, bytes)
            .map_err(|e| ExecError::Msg(e.to_string()))?;
        n += 1;
    }
    Ok(QueryResult {
        tag: format!("COPY {n}"),
        ..Default::default()
    })
}

pub fn copy_out(
    catalog: &SharedCatalog,
    namespace: &str,
    container: &str,
    format: CopyFormat,
) -> Result<QueryResult, ExecError> {
    let _ = format;
    let scan = select_rows(
        catalog,
        namespace,
        &format!("SELECT * FROM {container}"),
    )?;
    Ok(QueryResult {
        tag: format!("COPY {}", scan.rows.len()),
        columns: scan.columns,
        rows: scan.rows,
    })
}

fn cell(obj: &Value, col: &str) -> Option<String> {
    obj.get(col).and_then(|v| match v {
        Value::String(s) => Some(s.clone()),
        Value::Null => None,
        other => Some(other.to_string()),
    })
}

fn extract_table_name_after(upper: &str, original: &str, keyword: &str) -> Result<String, ExecError> {
    let kw = keyword.to_ascii_uppercase();
    let pos = upper
        .find(&kw)
        .ok_or_else(|| ExecError::Msg(format!("syntax: missing {keyword}")))?;
    let mut rest = original[pos + keyword.len()..].trim_start();
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
        return Err(ExecError::Msg("syntax: empty table name".into()));
    }
    Ok(name)
}

fn extract_ident_after_verb(sql: &str, verb: &str) -> Result<String, ExecError> {
    let upper = sql.to_ascii_uppercase();
    let v = verb.to_ascii_uppercase();
    if !upper.starts_with(&v) {
        return Err(ExecError::Msg(format!("expected {verb}")));
    }
    let rest = sql[verb.len()..].trim_start();
    Ok(rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect())
}

fn parse_column_list(sql: &str) -> Vec<String> {
    let start = match sql.find('(') {
        Some(i) => i + 1,
        None => return vec![],
    };
    let end = sql[start..].find(')').map(|i| start + i).unwrap_or(sql.len());
    sql[start..end]
        .split(',')
        .filter_map(|p| {
            let name = p.split_whitespace().next()?.trim_matches('"');
            if name.is_empty() {
                None
            } else {
                Some(name.to_string())
            }
        })
        .collect()
}

fn parse_values(sql: &str) -> Result<Vec<String>, ExecError> {
    let upper = sql.to_ascii_uppercase();
    let pos = upper
        .find("VALUES")
        .ok_or_else(|| ExecError::Msg("missing VALUES".into()))?;
    let after = &sql[pos + 6..];
    let start = after
        .find('(')
        .ok_or_else(|| ExecError::Msg("VALUES need (".into()))?
        + 1;
    let end = after[start..]
        .find(')')
        .ok_or_else(|| ExecError::Msg("VALUES need )".into()))?
        + start;
    Ok(after[start..end]
        .split(',')
        .map(|s| s.trim().trim_matches('\'').to_string())
        .collect())
}

fn parse_simple_where(sql: &str) -> Option<(String, String)> {
    let upper = sql.to_ascii_uppercase();
    let pos = upper.find(" WHERE ")?;
    let clause = sql[pos + 7..].trim();
    let mut parts = clause.splitn(2, '=');
    let k = parts.next()?.trim().to_string();
    let v = parts
        .next()?
        .trim()
        .trim_matches('\'')
        .split_whitespace()
        .next()?
        .trim_matches(';')
        .to_string();
    Some((k, v))
}

fn parse_set_clause(sql: &str) -> Result<Vec<(String, String)>, ExecError> {
    let upper = sql.to_ascii_uppercase();
    let pos = upper
        .find(" SET ")
        .ok_or_else(|| ExecError::Msg("missing SET".into()))?;
    let after = &sql[pos + 5..];
    let end = upper[pos + 5..]
        .find(" WHERE ")
        .map(|i| i)
        .unwrap_or(after.len());
    let body = &after[..end.min(after.len())];
    let mut out = Vec::new();
    for part in body.split(',') {
        let mut kv = part.splitn(2, '=');
        let k = kv
            .next()
            .ok_or_else(|| ExecError::Msg("bad SET".into()))?
            .trim()
            .to_string();
        let v = kv
            .next()
            .ok_or_else(|| ExecError::Msg("bad SET".into()))?
            .trim()
            .trim_matches('\'')
            .to_string();
        out.push((k, v));
    }
    Ok(out)
}

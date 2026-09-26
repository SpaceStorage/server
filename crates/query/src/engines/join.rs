//! Join engines — Inner / Left; HashJoin preferred (005 FR-028).

use crate::catalog_exec::{QueryResult, SharedCatalog};
use crate::error::ExecError;
use crate::request::JoinKind;
use serde_json::Value;
use std::collections::HashMap;

pub fn run(
    catalog: &SharedCatalog,
    namespace: &str,
    left: &str,
    right: &str,
    kind: JoinKind,
    left_key: &str,
    right_key: &str,
) -> Result<QueryResult, ExecError> {
    let cat = catalog.read().expect("catalog");
    let lc = cat
        .describe(namespace, left)
        .map_err(|e| ExecError::Msg(e.to_string()))?;
    let rc = cat
        .describe(namespace, right)
        .map_err(|e| ExecError::Msg(e.to_string()))?;
    let left_cols: Vec<String> = lc
        .schema
        .as_ref()
        .map(|s| s.fields.iter().map(|f| f.name.clone()).collect())
        .unwrap_or_default();
    let right_cols: Vec<String> = rc
        .schema
        .as_ref()
        .map(|s| s.fields.iter().map(|f| f.name.clone()).collect())
        .unwrap_or_default();

    // Build hash on right
    let mut build: HashMap<String, Vec<Value>> = HashMap::new();
    for k in cat.keys(rc.id).map_err(|e| ExecError::Msg(e.to_string()))? {
        if let Some(raw) = cat.get(rc.id, &k).ok().flatten() {
            let obj: Value = serde_json::from_slice(raw).unwrap_or(Value::Null);
            let key = obj
                .get(right_key)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            build.entry(key).or_default().push(obj);
        }
    }

    let mut out_cols = left_cols.clone();
    for c in &right_cols {
        out_cols.push(format!("{right}.{c}"));
    }
    let mut rows = Vec::new();
    for k in cat.keys(lc.id).map_err(|e| ExecError::Msg(e.to_string()))? {
        let Some(raw) = cat.get(lc.id, &k).ok().flatten() else {
            continue;
        };
        let lobj: Value = serde_json::from_slice(raw).unwrap_or(Value::Null);
        let lk = lobj
            .get(left_key)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        match build.get(&lk) {
            Some(matches) => {
                for robj in matches {
                    let mut row = Vec::new();
                    for c in &left_cols {
                        row.push(cell(&lobj, c));
                    }
                    for c in &right_cols {
                        row.push(cell(robj, c));
                    }
                    rows.push(row);
                }
            }
            None if kind == JoinKind::Left => {
                let mut row = Vec::new();
                for c in &left_cols {
                    row.push(cell(&lobj, c));
                }
                for _ in &right_cols {
                    row.push(None);
                }
                rows.push(row);
            }
            None => {}
        }
    }
    Ok(QueryResult {
        tag: format!("SELECT {}", rows.len()),
        columns: out_cols,
        rows,
    })
}

fn cell(obj: &Value, col: &str) -> Option<String> {
    obj.get(col).and_then(|v| match v {
        Value::String(s) => Some(s.clone()),
        Value::Null => None,
        other => Some(other.to_string()),
    })
}

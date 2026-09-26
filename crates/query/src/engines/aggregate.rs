//! Aggregation — COUNT/SUM/MIN/MAX/AVG + GROUP BY (005 FR-028).

use crate::catalog_exec::{QueryResult, SharedCatalog};
use crate::error::ExecError;
use crate::request::AggFn;
use serde_json::Value;
use std::collections::HashMap;

pub fn run(
    catalog: &SharedCatalog,
    namespace: &str,
    container: &str,
    func: AggFn,
    group_by: Option<&str>,
    column: Option<&str>,
) -> Result<QueryResult, ExecError> {
    let cat = catalog.read().expect("catalog");
    let c = cat
        .describe(namespace, container)
        .map_err(|e| ExecError::Msg(e.to_string()))?;
    let id = c.id;
    let col = column.unwrap_or("id");

    #[derive(Default)]
    struct Acc {
        count: u64,
        sum: f64,
        min: Option<f64>,
        max: Option<f64>,
    }

    let mut groups: HashMap<String, Acc> = HashMap::new();
    for k in cat.keys(id).map_err(|e| ExecError::Msg(e.to_string()))? {
        let Some(raw) = cat.get(id, &k).ok().flatten() else {
            continue;
        };
        let obj: Value = serde_json::from_slice(raw).unwrap_or(Value::Null);
        let gkey = group_by
            .and_then(|g| obj.get(g).and_then(|v| v.as_str()).map(|s| s.to_string()))
            .unwrap_or_else(|| "".into());
        let num = obj
            .get(col)
            .and_then(|v| match v {
                Value::Number(n) => n.as_f64(),
                Value::String(s) => s.parse().ok(),
                _ => None,
            })
            .unwrap_or(1.0);
        let acc = groups.entry(gkey).or_default();
        acc.count += 1;
        acc.sum += num;
        acc.min = Some(acc.min.map(|m| m.min(num)).unwrap_or(num));
        acc.max = Some(acc.max.map(|m| m.max(num)).unwrap_or(num));
    }

    let mut columns = Vec::new();
    if group_by.is_some() {
        columns.push(group_by.unwrap().to_string());
    }
    columns.push(format!("{func:?}").to_ascii_lowercase());

    let mut rows = Vec::new();
    for (g, acc) in groups {
        let val = match func {
            AggFn::Count => acc.count as f64,
            AggFn::Sum => acc.sum,
            AggFn::Min => acc.min.unwrap_or(0.0),
            AggFn::Max => acc.max.unwrap_or(0.0),
            AggFn::Avg => {
                if acc.count == 0 {
                    0.0
                } else {
                    acc.sum / acc.count as f64
                }
            }
        };
        let mut row = Vec::new();
        if group_by.is_some() {
            row.push(Some(g));
        }
        row.push(Some(format!("{val}")));
        rows.push(row);
    }
    Ok(QueryResult {
        tag: format!("SELECT {}", rows.len()),
        columns,
        rows,
    })
}

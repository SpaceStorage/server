//! MapReduce task emission (005 FR-026).

use crate::catalog_exec::{QueryResult, SharedCatalog};
use crate::error::ExecError;
use crate::engines::shuffle::{ShuffleJob, ShuffleTransport};
use serde_json::Value;

pub fn run(
    catalog: &SharedCatalog,
    namespace: &str,
    container: &str,
    map_expr: &str,
    reduce_expr: &str,
    shuffle: &ShuffleTransport,
) -> Result<QueryResult, ExecError> {
    let _ = (map_expr, reduce_expr);
    let job = ShuffleJob::new();
    shuffle.offer(&job, 0)?;
    let cat = catalog.read().expect("catalog");
    let c = cat
        .describe(namespace, container)
        .map_err(|e| ExecError::Msg(e.to_string()))?;
    let mut mapped = Vec::new();
    for k in cat.keys(c.id).map_err(|e| ExecError::Msg(e.to_string()))? {
        if let Some(raw) = cat.get(c.id, &k).ok().flatten() {
            let obj: Value = serde_json::from_slice(raw).unwrap_or(Value::Null);
            let bytes = serde_json::to_vec(&obj).unwrap_or_default();
            shuffle.push(&job, 0, &bytes)?;
            mapped.push(obj);
        }
    }
    let pulled = shuffle.pull(&job, 0)?;
    let _ = pulled;
    // Reduce: count keys
    let n = mapped.len();
    Ok(QueryResult {
        tag: format!("MAPREDUCE {n}"),
        columns: vec!["count".into()],
        rows: vec![vec![Some(n.to_string())]],
    })
}

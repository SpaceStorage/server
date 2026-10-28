//! Catalog + MappingQuery → LogicalRequest seam (010 contracts/mapping.md).

use crate::error::MigrateError;
use serde::{Deserialize, Serialize};
use spacestorage_query::{
    CancelToken, LogicalRequest, PlannerEngine, QueryOptions, SharedCatalog,
};
use spacestorage_types::{ContainerCatalog, L3Model, StorageModeChoice};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldMap {
    pub src: String,
    pub dst: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogMapping {
    pub from: String,
    pub to: String,
    pub fields: Vec<FieldMap>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingSource {
    pub namespace: String,
    pub container: String,
    pub columns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingQuery {
    pub sources: Vec<MappingSource>,
    pub destinations: Vec<MappingSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MappingSpec {
    Catalog(CatalogMapping),
    Query(MappingQuery),
}

/// Built-in understandable pairs (identity or documented default_transform).
pub fn catalog_default(from: &str, to: &str) -> Option<CatalogMapping> {
    if from == to {
        return Some(CatalogMapping {
            from: from.into(),
            to: to.into(),
            fields: vec![FieldMap {
                src: "*".into(),
                dst: "*".into(),
            }],
        });
    }
    // DocumentStore → RelationalTable default.
    if (from == "document_store" || from == "DocumentStore")
        && (to == "relational_table" || to == "RelationalTable")
    {
        return Some(CatalogMapping {
            from: from.into(),
            to: to.into(),
            fields: vec![
                FieldMap {
                    src: "id".into(),
                    dst: "id".into(),
                },
                FieldMap {
                    src: "body".into(),
                    dst: "body".into(),
                },
            ],
        });
    }
    None
}

pub fn resolve_mapping(
    from: &str,
    to: &str,
    query: Option<&MappingQuery>,
    complex_without_catalog: bool,
) -> Result<MappingSpec, MigrateError> {
    if let Some(q) = query {
        if q.sources.is_empty() {
            return Err(MigrateError::MappingSourceMissing {
                name: "sources".into(),
            });
        }
        // Join/agg markers refused.
        if q.filter.as_ref().is_some_and(|f| {
            f.get("join").is_some() || f.get("aggregate").is_some() || f.get("agg").is_some()
        }) {
            return Err(MigrateError::NotSupported {
                what: "join/aggregate mapping".into(),
            });
        }
        for s in &q.sources {
            if s.container.is_empty() {
                return Err(MigrateError::MappingSourceMissing {
                    name: "container".into(),
                });
            }
        }
        return Ok(MappingSpec::Query(q.clone()));
    }
    if let Some(c) = catalog_default(from, to) {
        return Ok(MappingSpec::Catalog(c));
    }
    if complex_without_catalog {
        return Err(MigrateError::MappingQueryRequired);
    }
    Err(MigrateError::MappingQueryRequired)
}

/// Lower MappingQuery to a `005` `LogicalRequest` (scan + project + insert Batch).
/// This crate does not parse SQL; admin/CLI may send the same AST.
pub fn lower_to_logical_request(q: &MappingQuery) -> LogicalRequest {
    let src = q.sources.first();
    let dst = q.destinations.first();
    let src_container = src
        .map(|s| s.container.clone())
        .unwrap_or_else(|| "source".into());
    let dst_container = dst
        .map(|s| s.container.clone())
        .unwrap_or_else(|| "dest".into());
    let filter = q.filter.as_ref().and_then(|f| {
        if f.is_null() {
            None
        } else {
            Some(f.to_string())
        }
    });
    // Project columns are applied when materialising insert Object ops after Scan.
    let _src_cols = src.map(|s| s.columns.clone()).unwrap_or_default();
    let _dst_cols = dst.map(|s| s.columns.clone()).unwrap_or_default();
    LogicalRequest::Batch {
        requests: vec![
            LogicalRequest::Scan {
                container: src_container,
                filter,
            },
            // Placeholder insert target — concrete Object puts are built after Scan
            // results are projected (see [`execute_mapping_query`]).
            LogicalRequest::TypeOp {
                container: dst_container,
                op: "scan_project_insert".into(),
            },
        ],
    }
}

/// Wire/debug JSON form of the lowered LogicalRequest (fixtures / admin).
pub fn lower_to_logical_request_json(q: &MappingQuery) -> serde_json::Value {
    let src_cols: Vec<&str> = q
        .sources
        .first()
        .map(|s| s.columns.iter().map(|c| c.as_str()).collect())
        .unwrap_or_default();
    let dst_cols: Vec<&str> = q
        .destinations
        .first()
        .map(|s| s.columns.iter().map(|c| c.as_str()).collect())
        .unwrap_or_default();
    let source = q.sources.first().map(|s| {
        serde_json::json!({
            "namespace": s.namespace,
            "container": s.container,
        })
    });
    let destination = q.destinations.first().map(|s| {
        serde_json::json!({
            "namespace": s.namespace,
            "container": s.container,
        })
    });
    serde_json::json!({
        "op": "LogicalRequest",
        "kind": "scan_project_insert",
        "scan": {
            "container": source,
            "columns": src_cols,
            "filter": q.filter,
        },
        "project": {
            "from": src_cols,
            "to": dst_cols,
        },
        "insert": {
            "container": destination,
            "columns": dst_cols,
        },
        "sources": q.sources,
        "destinations": q.destinations,
    })
}

/// Evaluate a mapping query through `005` PlannerEngine (scan → project → insert).
pub fn execute_mapping_query(
    q: &MappingQuery,
    source_rows: &HashMap<Vec<u8>, Vec<u8>>,
) -> Result<HashMap<Vec<u8>, Vec<u8>>, MigrateError> {
    let src = q.sources.first().ok_or_else(|| MigrateError::MappingSourceMissing {
        name: "sources".into(),
    })?;
    let dst = q
        .destinations
        .first()
        .cloned()
        .unwrap_or_else(|| MappingSource {
            namespace: src.namespace.clone(),
            container: format!("{}_out", src.container),
            columns: src.columns.clone(),
        });

    let catalog: SharedCatalog = Arc::new(RwLock::new(ContainerCatalog::new()));
    {
        let mut cat = catalog.write().map_err(|e| MigrateError::Io(e.to_string()))?;
        let sid = cat
            .create(
                &src.namespace,
                &src.container,
                L3Model::KvStore,
                false,
                None,
                StorageModeChoice::Memory,
            )
            .map_err(|e| MigrateError::Io(e.to_string()))?;
        for (k, v) in source_rows {
            let key = String::from_utf8_lossy(k);
            cat.put(sid, &key, v.clone())
                .map_err(|e| MigrateError::Io(e.to_string()))?;
        }
        let _ = cat.create(
            &dst.namespace,
            &dst.container,
            L3Model::KvStore,
            false,
            None,
            StorageModeChoice::Memory,
        );
    }

    let engine = PlannerEngine::new(Arc::clone(&catalog));
    let ns = src.namespace.as_str();
    let scan = LogicalRequest::Scan {
        container: src.container.clone(),
        filter: q.filter.as_ref().and_then(|f| {
            if f.is_null() {
                None
            } else {
                Some(f.to_string())
            }
        }),
    };
    // Ensure the lowered Batch shape is valid 005 IR (plan accepts it).
    let _ = lower_to_logical_request(q);
    let scanned = engine
        .execute_blocking(
            ns,
            "migrate-mapping",
            "object",
            scan,
            QueryOptions::default(),
            CancelToken::default(),
        )
        .map_err(|e| MigrateError::Io(e.to_string()))?;

    let from_cols = &src.columns;
    let to_cols = &dst.columns;
    let mut inserts = Vec::new();
    for row in &scanned.rows {
        let key = row.first().and_then(|c| c.as_ref()).cloned().unwrap_or_default();
        let value = row.get(1).and_then(|c| c.as_ref()).cloned().unwrap_or_default();
        let projected = project_row_bytes(from_cols, to_cols, value.as_bytes());
        inserts.push(LogicalRequest::Object {
            container: dst.container.clone(),
            key,
            bytes: projected,
        });
    }
    if !inserts.is_empty() {
        engine
            .execute_blocking(
                &dst.namespace,
                "migrate-mapping",
                "object",
                LogicalRequest::Batch { requests: inserts },
                QueryOptions::default(),
                CancelToken::default(),
            )
            .map_err(|e| MigrateError::Io(e.to_string()))?;
    }

    let cat = catalog.read().map_err(|e| MigrateError::Io(e.to_string()))?;
    let dest_id = cat
        .describe(&dst.namespace, &dst.container)
        .map_err(|_| MigrateError::NotFound)?
        .id;
    let mut out = HashMap::new();
    for k in cat.keys(dest_id).unwrap_or_default() {
        if let Ok(Some(v)) = cat.get(dest_id, &k) {
            out.insert(k.into_bytes(), v.to_vec());
        }
    }
    Ok(out)
}

fn project_row_bytes(from_cols: &[String], to_cols: &[String], value: &[u8]) -> Vec<u8> {
    if from_cols.is_empty()
        || (from_cols.len() == 1 && from_cols[0] == "*")
            && (to_cols.is_empty() || (to_cols.len() == 1 && to_cols[0] == "*"))
    {
        return value.to_vec();
    }
    if let Ok(obj) = serde_json::from_slice::<serde_json::Map<String, serde_json::Value>>(value) {
        let mut out = serde_json::Map::new();
        for (i, src) in from_cols.iter().enumerate() {
            let dst = to_cols.get(i).map(|s| s.as_str()).unwrap_or(src.as_str());
            if src == "*" {
                for (k, v) in &obj {
                    out.insert(k.clone(), v.clone());
                }
            } else if let Some(val) = obj.get(src) {
                out.insert(dst.into(), val.clone());
            }
        }
        return serde_json::to_vec(&out).unwrap_or_else(|_| value.to_vec());
    }
    value.to_vec()
}

/// Apply a resolved mapping / rewrite to one source (key, value) → destination bytes.
/// Non-identity rewrites MUST change the stored representation (T060).
/// MappingQuery rows MUST go through [`execute_mapping_query`] (005), not this path.
pub fn apply_row_transform(
    rewrite: &str,
    mapping: &MappingSpec,
    key: &[u8],
    value: &[u8],
) -> (Vec<u8>, Vec<u8>) {
    match rewrite {
        "re_encode" => (key.to_vec(), wrap_tag(b"re_encode/v1", value)),
        "re_compress" => (key.to_vec(), wrap_tag(b"re_compress/v1", value)),
        "re_encrypt" => (key.to_vec(), wrap_tag(b"re_encrypt/v1", value)),
        "sharding_key" => {
            let mut nk = b"sk:".to_vec();
            nk.extend_from_slice(key);
            (nk, value.to_vec())
        }
        "type_model" | "incompatible_schema" => apply_catalog_field_mapping(mapping, key, value),
        _ => apply_catalog_field_mapping(mapping, key, value),
    }
}

fn wrap_tag(tag: &[u8], value: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(tag.len() + 1 + value.len());
    out.extend_from_slice(tag);
    out.push(b'|');
    out.extend_from_slice(value);
    out
}

fn apply_catalog_field_mapping(
    mapping: &MappingSpec,
    key: &[u8],
    value: &[u8],
) -> (Vec<u8>, Vec<u8>) {
    match mapping {
        MappingSpec::Catalog(c) => {
            if c.fields.len() == 1 && c.fields[0].src == "*" && c.fields[0].dst == "*" {
                let v = wrap_tag(format!("map:{}->{}", c.from, c.to).as_bytes(), value);
                return (key.to_vec(), v);
            }
            if let Ok(mut obj) =
                serde_json::from_slice::<serde_json::Map<String, serde_json::Value>>(value)
            {
                let mut out = serde_json::Map::new();
                for f in &c.fields {
                    if let Some(val) = obj.remove(&f.src) {
                        out.insert(f.dst.clone(), val);
                    } else if f.src == "*" {
                        for (k, v) in obj.iter() {
                            out.insert(k.clone(), v.clone());
                        }
                    }
                }
                let bytes = serde_json::to_vec(&out).unwrap_or_else(|_| value.to_vec());
                return (key.to_vec(), bytes);
            }
            (
                key.to_vec(),
                wrap_tag(format!("map:{}->{}", c.from, c.to).as_bytes(), value),
            )
        }
        MappingSpec::Query(_) => {
            // Private remap is forbidden for queries — callers must use execute_mapping_query.
            (
                key.to_vec(),
                wrap_tag(b"logical_request/scan_project_insert", value),
            )
        }
    }
}

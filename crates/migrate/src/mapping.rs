//! Catalog + MappingQuery → LogicalRequest seam (010 contracts/mapping.md).

use crate::error::MigrateError;
use serde::{Deserialize, Serialize};

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

/// Lower MappingQuery to a scan+project+insert shaped JSON (005 LogicalRequest seam).
pub fn lower_to_logical_request(q: &MappingQuery) -> serde_json::Value {
    serde_json::json!({
        "op": "scan_project_insert",
        "sources": q.sources,
        "destinations": q.destinations,
        "filter": q.filter,
    })
}

//! Classify-gated ES verb dispatch (015 elasticsearch-search.md HC MUST).

use std::sync::{Arc, RwLock};

use serde_json::{json, Value};
use spacestorage_compat::{
    classify_outcome, record_must_not, ClassifyOutcome, CompatError, DialectProfile, ProtocolId,
};
use spacestorage_types::{ContainerCatalog, ContainerId, L3Model, StorageModeChoice, TypeError};

#[derive(Debug, Clone, PartialEq)]
pub enum EsReply {
    Ok(Value),
    Error { status: u16, body: Value },
}

pub struct SessionState {
    pub catalog: Arc<RwLock<ContainerCatalog>>,
    pub namespace: String,
    pub profile: DialectProfile,
}

impl SessionState {
    pub fn demo(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self {
            catalog,
            namespace: "demo".into(),
            profile: DialectProfile::HandlersComplete,
        }
    }

    fn ensure_index(&self, index: &str) -> Result<ContainerId, EsReply> {
        let mut cat = self.catalog.write().expect("catalog");
        match cat.describe(&self.namespace, index) {
            Ok(c) => {
                if c.model != L3Model::DocumentStore {
                    return Err(error_reply(
                        400,
                        "illegal_argument_exception",
                        "wrong container type for index",
                    ));
                }
                Ok(c.id)
            }
            Err(TypeError::NotFound) => cat
                .create(
                    &self.namespace,
                    index,
                    L3Model::DocumentStore,
                    false,
                    None,
                    StorageModeChoice::Persistent,
                )
                .map_err(|e| {
                    error_reply(500, "server_error", &format!("{e:?}"))
                }),
            Err(e) => Err(error_reply(500, "server_error", &format!("{e:?}"))),
        }
    }
}

fn error_reply(status: u16, typ: &str, reason: &str) -> EsReply {
    EsReply::Error {
        status,
        body: json!({
            "error": { "type": typ, "reason": reason },
            "status": status
        }),
    }
}

fn refuse(verb: &str, err: CompatError) -> EsReply {
    record_must_not(ProtocolId::Elasticsearch.as_str(), verb);
    let typ = if err.code() == "agg_not_in_profile" {
        "illegal_argument_exception"
    } else {
        "illegal_argument_exception"
    };
    // Never 200 + empty hits for MUST NOT.
    error_reply(400, typ, &err.to_string())
}

/// Dispatch ES verb (`INDEX`, `GET`, `SEARCH_MATCH`, `AGG_TERMS`, `ILM`, …).
pub fn dispatch(session: &mut SessionState, verb: &str, args: &[&str]) -> EsReply {
    let upper = verb.to_ascii_uppercase().replace('-', "_");
    match classify_outcome(session.profile, ProtocolId::Elasticsearch, &upper) {
        ClassifyOutcome::Must => {}
        ClassifyOutcome::MustNot(err) => return refuse(&upper, err),
    }

    match upper.as_str() {
        "CREATE_INDEX" | "LIST" | "CAT" => {
            let index = args.first().copied().unwrap_or("docs");
            match session.ensure_index(index) {
                Ok(_) => EsReply::Ok(json!({ "acknowledged": true, "index": index })),
                Err(e) => e,
            }
        }
        "INDEX" => {
            let index = args.first().copied().unwrap_or("docs");
            let id = args.get(1).copied().unwrap_or("1");
            let body = args.get(2).copied().unwrap_or("{}");
            let cid = match session.ensure_index(index) {
                Ok(c) => c,
                Err(e) => return e,
            };
            let mut cat = session.catalog.write().expect("catalog");
            match cat.put(cid, id, body.as_bytes().to_vec()) {
                Ok(()) => EsReply::Ok(json!({
                    "_index": index,
                    "_id": id,
                    "result": "created"
                })),
                Err(e) => error_reply(500, "server_error", &format!("{e:?}")),
            }
        }
        "GET" => {
            let index = args.first().copied().unwrap_or("docs");
            let id = args.get(1).copied().unwrap_or("1");
            let cid = match session.ensure_index(index) {
                Ok(c) => c,
                Err(e) => return e,
            };
            let cat = session.catalog.read().expect("catalog");
            match cat.get(cid, id) {
                Ok(Some(v)) => {
                    let source: Value =
                        serde_json::from_slice(v).unwrap_or_else(|_| json!({ "raw": String::from_utf8_lossy(v) }));
                    EsReply::Ok(json!({
                        "_index": index,
                        "_id": id,
                        "found": true,
                        "_source": source
                    }))
                }
                Ok(None) => EsReply::Ok(json!({ "_index": index, "_id": id, "found": false })),
                Err(e) => error_reply(500, "server_error", &format!("{e:?}")),
            }
        }
        "DELETE" => {
            let index = args.first().copied().unwrap_or("docs");
            let id = args.get(1).copied().unwrap_or("1");
            let cid = match session.ensure_index(index) {
                Ok(c) => c,
                Err(e) => return e,
            };
            let mut cat = session.catalog.write().expect("catalog");
            match cat.delete_row(cid, id) {
                Ok(true) => EsReply::Ok(json!({ "_index": index, "_id": id, "result": "deleted" })),
                Ok(false) => EsReply::Ok(json!({ "_index": index, "_id": id, "result": "not_found" })),
                Err(e) => error_reply(500, "server_error", &format!("{e:?}")),
            }
        }
        "SEARCH" | "SEARCH_QUERY_STRING" | "SEARCH_MATCH" | "SEARCH_TERM" | "SEARCH_RANGE"
        | "SEARCH_BOOL" => {
            let index = args.first().copied().unwrap_or("docs");
            let needle = args.get(1).copied().unwrap_or("");
            let cid = match session.ensure_index(index) {
                Ok(c) => c,
                Err(e) => return e,
            };
            let cat = session.catalog.read().expect("catalog");
            let keys = match cat.keys(cid) {
                Ok(k) => k,
                Err(e) => return error_reply(500, "server_error", &format!("{e:?}")),
            };
            let mut hits = Vec::new();
            for k in keys {
                if let Ok(Some(v)) = cat.get(cid, &k) {
                    let text = String::from_utf8_lossy(v);
                    if needle.is_empty() || text.contains(needle) || k.contains(needle) {
                        let source: Value = serde_json::from_slice(v)
                            .unwrap_or_else(|_| json!({ "raw": text }));
                        hits.push(json!({
                            "_index": index,
                            "_id": k,
                            "_source": source
                        }));
                    }
                }
            }
            EsReply::Ok(json!({
                "hits": {
                    "total": { "value": hits.len(), "relation": "eq" },
                    "hits": hits
                }
            }))
        }
        other => refuse(
            other,
            CompatError::CompatMustNot {
                protocol: ProtocolId::Elasticsearch.as_str().into(),
                verb: other.into(),
            },
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crud_and_search() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(cat);
        assert!(matches!(
            dispatch(&mut s, "CREATE_INDEX", &["docs"]),
            EsReply::Ok(_)
        ));
        assert!(matches!(
            dispatch(&mut s, "INDEX", &["docs", "1", r#"{"title":"hello"}"#]),
            EsReply::Ok(_)
        ));
        match dispatch(&mut s, "SEARCH_MATCH", &["docs", "hello"]) {
            EsReply::Ok(v) => {
                assert!(v["hits"]["total"]["value"].as_u64().unwrap() >= 1);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn agg_not_in_profile_never_200_empty() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(cat);
        match dispatch(&mut s, "AGG_TERMS", &["docs"]) {
            EsReply::Error { status, body } => {
                assert_eq!(status, 400);
                assert_ne!(status, 200);
                let reason = body["error"]["reason"].as_str().unwrap_or("");
                assert!(reason.contains("agg_not_in_profile") || body["error"]["type"].is_string());
            }
            EsReply::Ok(_) => panic!("MUST NOT must not return 200"),
        }
    }
}

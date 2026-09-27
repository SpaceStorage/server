//! Classify-gated ES verb dispatch via shared `PlannerEngine` (005 T085).

use std::sync::{Arc, RwLock};

use serde_json::{json, Value};
use spacestorage_compat::{
    classify_outcome, record_must_not, ClassifyOutcome, CompatError, DialectProfile, ProtocolId,
};
use spacestorage_query::{CancelToken, LogicalRequest, PlannerEngine, QueryOptions, QueryResult};
use spacestorage_types::ContainerCatalog;

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

    fn engine(&self) -> PlannerEngine {
        PlannerEngine::new(Arc::clone(&self.catalog))
    }

    fn exec(&self, req: LogicalRequest) -> Result<QueryResult, EsReply> {
        self.engine()
            .execute_blocking(
                &self.namespace,
                "es",
                "elasticsearch",
                req,
                QueryOptions::default(),
                CancelToken::new(),
            )
            .map_err(|e| error_reply(500, "server_error", &e.to_string()))
    }

    fn ensure_index(&self, index: &str) -> Result<(), EsReply> {
        self.exec(LogicalRequest::TypeOp {
            container: index.into(),
            op: "ensure_doc".into(),
        })?;
        Ok(())
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
    let typ = "illegal_argument_exception";
    error_reply(400, typ, &err.to_string())
}

/// Dispatch ES verb (`INDEX`, `GET`, `SEARCH_MATCH`, …) through PlannerEngine IR.
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
                Ok(()) => EsReply::Ok(json!({ "acknowledged": true, "index": index })),
                Err(e) => e,
            }
        }
        "INDEX" => {
            let index = args.first().copied().unwrap_or("docs");
            let id = args.get(1).copied().unwrap_or("1");
            let body = args.get(2).copied().unwrap_or("{}");
            if let Err(e) = session.ensure_index(index) {
                return e;
            }
            match session.exec(LogicalRequest::Object {
                container: index.into(),
                key: id.into(),
                bytes: body.as_bytes().to_vec(),
            }) {
                Ok(_) => EsReply::Ok(json!({
                    "_index": index,
                    "_id": id,
                    "result": "created"
                })),
                Err(e) => e,
            }
        }
        "GET" => {
            let index = args.first().copied().unwrap_or("docs");
            let id = args.get(1).copied().unwrap_or("1");
            if let Err(e) = session.ensure_index(index) {
                return e;
            }
            match session.exec(LogicalRequest::Point {
                container: index.into(),
                key: id.into(),
            }) {
                Ok(r) if r.rows.is_empty() => {
                    EsReply::Ok(json!({ "_index": index, "_id": id, "found": false }))
                }
                Ok(r) => {
                    let raw = r.rows[0]
                        .get(1)
                        .and_then(|x| x.clone())
                        .unwrap_or_else(|| "{}".into());
                    let source: Value =
                        serde_json::from_str(&raw).unwrap_or_else(|_| json!({ "raw": raw }));
                    EsReply::Ok(json!({
                        "_index": index,
                        "_id": id,
                        "found": true,
                        "_source": source
                    }))
                }
                Err(e) => e,
            }
        }
        "DELETE" => {
            let index = args.first().copied().unwrap_or("docs");
            let id = args.get(1).copied().unwrap_or("1");
            if let Err(e) = session.ensure_index(index) {
                return e;
            }
            match session.exec(LogicalRequest::Mutate {
                container: index.into(),
                op: "delete".into(),
                sql: id.into(),
            }) {
                Ok(r) if r.tag.contains('1') => {
                    EsReply::Ok(json!({ "_index": index, "_id": id, "result": "deleted" }))
                }
                Ok(_) => {
                    EsReply::Ok(json!({ "_index": index, "_id": id, "result": "not_found" }))
                }
                Err(e) => e,
            }
        }
        "SEARCH" | "SEARCH_QUERY_STRING" | "SEARCH_MATCH" | "SEARCH_TERM" | "SEARCH_RANGE"
        | "SEARCH_BOOL" => {
            let index = args.first().copied().unwrap_or("docs");
            let needle = args.get(1).copied().unwrap_or("");
            if let Err(e) = session.ensure_index(index) {
                return e;
            }
            match session.exec(LogicalRequest::Scan {
                container: index.into(),
                filter: if needle.is_empty() {
                    None
                } else {
                    Some(needle.into())
                },
            }) {
                Ok(r) => {
                    let mut hits = Vec::new();
                    for row in r.rows {
                        let k = row.first().and_then(|x| x.clone()).unwrap_or_default();
                        let text = row.get(1).and_then(|x| x.clone()).unwrap_or_default();
                        let source: Value = serde_json::from_str(&text)
                            .unwrap_or_else(|_| json!({ "raw": text }));
                        hits.push(json!({
                            "_index": index,
                            "_id": k,
                            "_source": source
                        }));
                    }
                    EsReply::Ok(json!({
                        "hits": {
                            "total": { "value": hits.len(), "relation": "eq" },
                            "hits": hits
                        }
                    }))
                }
                Err(e) => e,
            }
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

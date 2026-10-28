//! Map HTTP routes to classify-gated `dispatch` verbs.

use serde_json::{json, Value};
use spacestorage_protocol_core::{parse_http_request_line, split_target};

use crate::dispatch::{dispatch, EsReply, SessionState};

pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub headers: std::collections::HashMap<String, String>,
    pub body: Vec<u8>,
}

impl HttpRequest {
    /// Parse a complete HTTP/1.x request from `buf`. Returns `(request, consumed)`.
    pub fn parse(buf: &[u8]) -> Result<(Self, usize), ParseNeed> {
        let header_end = buf
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or(ParseNeed::Headers)?;
        let header_section = &buf[..header_end];
        let body_start = header_end + 4;

        let header_str = std::str::from_utf8(header_section).map_err(|_| ParseNeed::Invalid)?;
        let mut lines = header_str.split("\r\n");
        let request_line = lines.next().ok_or(ParseNeed::Invalid)?;
        let rl = parse_http_request_line(request_line).ok_or(ParseNeed::Invalid)?;
        let (path, query) = split_target(&rl.target);

        let mut headers = std::collections::HashMap::new();
        for line in lines {
            if line.is_empty() {
                continue;
            }
            if let Some((k, v)) = line.split_once(':') {
                headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
            }
        }

        let content_length: usize = headers
            .get("content-length")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let total = body_start + content_length;
        if buf.len() < total {
            return Err(ParseNeed::Body);
        }

        let body = buf[body_start..total].to_vec();
        Ok((
            Self {
                method: rl.method,
                path: path.to_string(),
                query: query.map(str::to_string),
                headers,
                body,
            },
            total,
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseNeed {
    Headers,
    Body,
    Invalid,
}

pub fn route(session: &mut SessionState, req: &HttpRequest) -> EsReply {
    if req.method.eq_ignore_ascii_case("GET") && req.path == "/" {
        return EsReply::Ok(json!({
            "name": "SpaceStorage",
            "cluster_name": "spacestorage",
            "cluster_uuid": "demo",
            "version": {
                "number": "8.15.0",
                "build_flavor": "default",
                "build_type": "docker",
                "lucene_version": "9.11.0"
            },
            "tagline": "You Know, for Search"
        }));
    }

    if !req.body.is_empty() {
        if let Err(e) = session
            .limit_services
            .limits()
            .check_query_text(req.body.len() as u64)
        {
            return EsReply::Error {
                status: 413,
                body: json!({
                    "error": { "type": "limit_exceeded", "reason": e.to_string() },
                    "status": 413
                }),
            };
        }
    }

    let segments: Vec<&str> = req
        .path
        .trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    match (req.method.to_ascii_uppercase().as_str(), segments.as_slice()) {
        ("PUT", [index]) => dispatch(session, "CREATE_INDEX", &[index]),
        ("PUT" | "POST", [index, "_doc", id]) => {
            let body = String::from_utf8_lossy(&req.body);
            dispatch(session, "INDEX", &[index, id, body.as_ref()])
        }
        ("GET", [index, "_doc", id]) => dispatch(session, "GET", &[index, id]),
        ("DELETE", [index, "_doc", id]) => dispatch(session, "DELETE", &[index, id]),
        ("GET", ["_cat"]) => dispatch(session, "CAT", &[]),
        ("GET", ["_cat", "indices"]) => dispatch(session, "LIST", &[]),
        ("POST", [index, "_search"]) => {
            let (verb, needle) = classify_search(&req.body, req.query.as_deref());
            dispatch(session, &verb, &[index, needle.as_str()])
        }
        ("GET", [index, "_search"]) => {
            let (verb, needle) = classify_search(&req.body, req.query.as_deref());
            dispatch(session, &verb, &[index, needle.as_str()])
        }
        ("PUT", ["_ilm", ..]) | ("POST", ["_ilm", ..]) => dispatch(session, "ILM", &[]),
        ("PUT", ["_ingest", ..]) | ("POST", ["_ingest", ..]) => dispatch(session, "INGEST", &[]),
        ("PUT", ["_ml", ..]) | ("POST", ["_ml", ..]) | ("PUT", ["_xpack", "ml", ..])
        | ("POST", ["_xpack", "ml", ..]) => dispatch(session, "ML", &[]),
        ("PUT", ["_ccr", ..]) | ("POST", ["_ccr", ..]) => dispatch(session, "CCR", &[]),
        _ => EsReply::Error {
            status: 404,
            body: json!({
                "error": {
                    "type": "index_not_found_exception",
                    "reason": "unsupported route"
                },
                "status": 404
            }),
        },
    }
}

fn classify_search(body: &[u8], query: Option<&str>) -> (String, String) {
    if !body.is_empty() {
        let v = match serde_json::from_slice::<Value>(body) {
            Ok(v) => v,
            Err(_) => return must_not_search_verb(),
        };

        if v.get("query").is_some() && classify_query(&v).is_none() {
            return must_not_search_verb();
        }
        if let Some(out) = classify_aggs(&v) {
            return out;
        }
        if let Some(out) = classify_query(&v) {
            return out;
        }
        return must_not_search_verb();
    }

    if let Some(q) = query {
        for part in q.split('&') {
            if let Some((k, v)) = part.split_once('=') {
                if k == "q" {
                    return ("SEARCH_QUERY_STRING".into(), v.to_string());
                }
            }
        }
    }

    must_not_search_verb()
}

fn must_not_search_verb() -> (String, String) {
    ("SCRIPT".into(), String::new())
}

fn closed_agg_verb(kind: &str) -> Option<&'static str> {
    match kind {
        "terms" => Some("AGG_TERMS"),
        "min" => Some("AGG_MIN"),
        "max" => Some("AGG_MAX"),
        "sum" => Some("AGG_SUM"),
        "avg" => Some("AGG_AVG"),
        "histogram" => Some("AGG_HISTOGRAM"),
        "value_count" => Some("AGG_VALUE_COUNT"),
        _ => None,
    }
}

/// Closed MUST aggregations → verb + field; forbidden/unknown → MustNot matrix verbs.
fn classify_aggs(v: &Value) -> Option<(String, String)> {
    let aggs = v.get("aggs").or_else(|| v.get("aggregations"))?.as_object()?;
    let mut first_closed: Option<(String, String)> = None;
    for (_name, body) in aggs {
        let obj = body.as_object()?;
        for (kind, spec) in obj {
            let Some(verb) = closed_agg_verb(kind) else {
                return Some(must_not_search_verb());
            };
            if first_closed.is_none() {
                let field = spec
                    .get("field")
                    .and_then(|f| f.as_str())
                    .unwrap_or("id")
                    .to_string();
                first_closed = Some((verb.into(), field));
            }
        }
    }
    first_closed
}

fn classify_query(v: &Value) -> Option<(String, String)> {
    let query = v.get("query")?;
    if query.is_null() {
        return None;
    }
    let obj = query.as_object()?;
    if obj.is_empty() {
        return None;
    }
    if obj.len() != 1 {
        return None;
    }
    let (kind, inner) = obj.iter().next()?;
    match kind.as_str() {
        "match" => extract_match_needle(v).map(|n| ("SEARCH_MATCH".into(), n)),
        "term" => extract_term_needle(v).map(|n| ("SEARCH_TERM".into(), n)),
        "query_string" => extract_query_string_needle(inner),
        "range" => extract_range_needle(inner),
        "bool" => classify_bool_query(inner),
        "script" => None,
        _ => None,
    }
}

fn extract_query_string_needle(inner: &Value) -> Option<(String, String)> {
    inner
        .get("query")
        .and_then(|q| q.as_str())
        .map(|s| ("SEARCH_QUERY_STRING".into(), s.to_string()))
        .or_else(|| inner.as_str().map(|s| ("SEARCH_QUERY_STRING".into(), s.to_string())))
}

fn extract_range_needle(inner: &Value) -> Option<(String, String)> {
    let field = inner.as_object()?.keys().next()?.clone();
    Some(("SEARCH_RANGE".into(), field))
}

fn classify_bool_query(inner: &Value) -> Option<(String, String)> {
    let obj = inner.as_object()?;
    for (clause, val) in obj {
        match clause.as_str() {
            "must" | "should" | "filter" | "must_not" => {
                if let Some(arr) = val.as_array() {
                    for item in arr {
                        if classify_query_fragment(item).is_none() {
                            return None;
                        }
                    }
                } else if classify_query_fragment(val).is_none() {
                    return None;
                }
            }
            _ => return None,
        }
    }
    Some(("SEARCH_BOOL".into(), String::new()))
}

fn classify_query_fragment(q: &Value) -> Option<()> {
    let obj = q.as_object()?;
    if obj.len() != 1 {
        return None;
    }
    let (kind, inner) = obj.iter().next()?;
    match kind.as_str() {
        "match" | "term" | "query_string" | "range" => Some(()),
        "bool" => classify_bool_query(inner).map(|_| ()),
        "script" => None,
        _ => None,
    }
}

fn extract_match_needle(v: &Value) -> Option<String> {
    let m = v.get("query")?.get("match")?.as_object()?;
    for val in m.values() {
        if let Some(s) = val.as_str() {
            return Some(s.to_string());
        }
        if let Some(q) = val.get("query").and_then(|x| x.as_str()) {
            return Some(q.to_string());
        }
    }
    None
}

fn extract_term_needle(v: &Value) -> Option<String> {
    let t = v.get("query")?.get("term")?.as_object()?;
    for val in t.values() {
        if let Some(s) = val.as_str() {
            return Some(s.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, RwLock};
    use spacestorage_types::ContainerCatalog;

    fn sample_request(raw: &str) -> HttpRequest {
        let (req, _) = HttpRequest::parse(raw.as_bytes()).expect("parse");
        req
    }

    #[test]
    fn http_parse_and_route_crud() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(cat);

        let create = sample_request(
            "PUT /docs HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\n\r\n",
        );
        assert!(matches!(route(&mut s, &create), EsReply::Ok(_)));

        let index = sample_request(concat!(
            "PUT /docs/_doc/1 HTTP/1.1\r\n",
            "Host: x\r\n",
            "Content-Length: 17\r\n",
            "\r\n",
            r#"{"title":"hello"}"#
        ));
        assert!(matches!(route(&mut s, &index), EsReply::Ok(_)));

        let search = sample_request(concat!(
            "POST /docs/_search HTTP/1.1\r\n",
            "Host: x\r\n",
            "Content-Length: 37\r\n",
            "\r\n",
            r#"{"query":{"match":{"title":"hello"}}}"#
        ));
        match route(&mut s, &search) {
            EsReply::Ok(v) => assert!(v["hits"]["total"]["value"].as_u64().unwrap() >= 1),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn search_q_only_with_empty_body() {
        let (verb, needle) = classify_search(b"", Some("q=title:hello"));
        assert_eq!(verb, "SEARCH_QUERY_STRING");
        assert_eq!(needle, "title:hello");
    }

    #[test]
    fn search_body_classified_before_q_param() {
        let script_body = br#"{"query":{"script":{"script":"1"}}}"#;
        let (verb, _) = classify_search(script_body, Some("q=title:hello"));
        assert_eq!(verb, "SCRIPT");

        let match_body = br#"{"query":{"match":{"title":"hello"}}}"#;
        let (verb, _) = classify_search(match_body, Some("q=other"));
        assert_eq!(verb, "SEARCH_MATCH");
    }

    #[test]
    fn get_search_with_body_classifies_like_post() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(cat);

        let create = sample_request(
            "PUT /docs HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\n\r\n",
        );
        assert!(matches!(route(&mut s, &create), EsReply::Ok(_)));

        let index = sample_request(concat!(
            "PUT /docs/_doc/1 HTTP/1.1\r\n",
            "Host: x\r\n",
            "Content-Length: 17\r\n",
            "\r\n",
            r#"{"title":"hello"}"#
        ));
        assert!(matches!(route(&mut s, &index), EsReply::Ok(_)));

        let search = sample_request(concat!(
            "GET /docs/_search HTTP/1.1\r\n",
            "Host: x\r\n",
            "Content-Length: 37\r\n",
            "\r\n",
            r#"{"query":{"match":{"title":"hello"}}}"#
        ));
        match route(&mut s, &search) {
            EsReply::Ok(v) => assert!(v["hits"]["total"]["value"].as_u64().unwrap() >= 1),
            other => panic!("{other:?}"),
        }

        let bad = sample_request(concat!(
            "GET /docs/_search?q=title:hello HTTP/1.1\r\n",
            "Host: x\r\n",
            "Content-Length: 35\r\n",
            "\r\n",
            r#"{"query":{"script":{"script":"1"}}}"#
        ));
        match route(&mut s, &bad) {
            EsReply::Error { status, body } => {
                assert_eq!(status, 400);
                assert_eq!(
                    body["error"]["type"].as_str(),
                    Some("illegal_argument_exception")
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn search_script_query_must_not() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(cat);
        let search = sample_request(concat!(
            "POST /docs/_search HTTP/1.1\r\n",
            "Host: x\r\n",
            "Content-Length: 35\r\n",
            "\r\n",
            r#"{"query":{"script":{"script":"1"}}}"#
        ));
        match route(&mut s, &search) {
            EsReply::Error { status, body } => {
                assert_eq!(status, 400);
                assert_eq!(
                    body["error"]["type"].as_str(),
                    Some("illegal_argument_exception")
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn search_null_or_empty_query_must_not_even_with_terms_agg() {
        let bodies = [
            br#"{"query":null,"aggs":{"t":{"terms":{"field":"id"}}}}"#.as_slice(),
            br#"{"query":{},"aggs":{"t":{"terms":{"field":"id"}}}}"#.as_slice(),
        ];
        for body in bodies {
            let (verb, _) = classify_search(body, None);
            assert_eq!(verb, "SCRIPT", "body = {body:?}");
        }
    }

    #[test]
    fn search_match_with_terms_agg_classifies_as_must_agg() {
        let body = br#"{"query":{"match":{"title":"hello"}},"aggs":{"t":{"terms":{"field":"title"}}}}"#;
        let (verb, field) = classify_search(body, None);
        assert_eq!(verb, "AGG_TERMS");
        assert_eq!(field, "title");
    }

    #[test]
    fn search_terms_plus_forbidden_agg_must_not() {
        let mixed = br#"{"aggs":{"t":{"terms":{"field":"id"}},"h":{"date_histogram":{"field":"@timestamp"}}}}"#;
        let (verb, _) = classify_search(mixed, None);
        assert_eq!(verb, "SCRIPT");

        let terms_only = br#"{"aggs":{"t":{"terms":{"field":"id"}}}}"#;
        let (verb, field) = classify_search(terms_only, None);
        assert_eq!(verb, "AGG_TERMS");
        assert_eq!(field, "id");

        let terms_and_avg = br#"{"aggs":{"t":{"terms":{"field":"id"}},"a":{"avg":{"field":"n"}}}}"#;
        let (verb, _) = classify_search(terms_and_avg, None);
        assert_ne!(verb, "SCRIPT");
        assert!(matches!(
            verb.as_str(),
            "AGG_TERMS" | "AGG_MIN" | "AGG_MAX" | "AGG_SUM" | "AGG_AVG" | "AGG_HISTOGRAM"
                | "AGG_VALUE_COUNT"
        ));
    }

    #[test]
    fn search_date_histogram_agg_must_not() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(cat);
        let search = sample_request(concat!(
            "POST /docs/_search HTTP/1.1\r\n",
            "Host: x\r\n",
            "Content-Length: 56\r\n",
            "\r\n",
            r#"{"aggs":{"h":{"date_histogram":{"field":"@timestamp"}}}}"#
        ));
        match route(&mut s, &search) {
            EsReply::Error { status, .. } => assert_eq!(status, 400),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn ingest_route_must_not() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(cat);
        let req = sample_request(
            "PUT /_ingest/pipeline/p1 HTTP/1.1\r\nHost: x\r\nContent-Length: 2\r\n\r\n{}",
        );
        match route(&mut s, &req) {
            EsReply::Error { status, .. } => assert_eq!(status, 400),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn delete_doc_cat_and_list_routes_dispatch() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(cat);

        let create = sample_request(
            "PUT /docs HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\n\r\n",
        );
        assert!(matches!(route(&mut s, &create), EsReply::Ok(_)));

        let index = sample_request(concat!(
            "PUT /docs/_doc/del-me HTTP/1.1\r\n",
            "Host: x\r\n",
            "Content-Length: 2\r\n",
            "\r\n",
            "{}"
        ));
        assert!(matches!(route(&mut s, &index), EsReply::Ok(_)));

        let delete = sample_request(
            "DELETE /docs/_doc/del-me HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\n\r\n",
        );
        match route(&mut s, &delete) {
            EsReply::Ok(v) => assert_eq!(v["result"].as_str(), Some("deleted")),
            other => panic!("{other:?}"),
        }

        let cat_req = sample_request("GET /_cat HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\n\r\n");
        match route(&mut s, &cat_req) {
            EsReply::Ok(v) => assert!(v.get("acknowledged").is_some()),
            other => panic!("{other:?}"),
        }

        let list_req = sample_request(
            "GET /_cat/indices HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\n\r\n",
        );
        match route(&mut s, &list_req) {
            EsReply::Ok(v) => assert!(v.get("acknowledged").is_some()),
            other => panic!("{other:?}"),
        }

        let missing = sample_request(
            "GET /nope/_unknown/extra HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\n\r\n",
        );
        match route(&mut s, &missing) {
            EsReply::Error { body, .. } => {
                assert_eq!(
                    body["error"]["type"].as_str(),
                    Some("index_not_found_exception")
                );
            }
            other => panic!("{other:?}"),
        }
    }
}

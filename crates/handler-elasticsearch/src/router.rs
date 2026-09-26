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
        ("POST", [index, "_search"]) => {
            let (verb, needle) = classify_search(&req.body, req.query.as_deref());
            dispatch(session, &verb, &[index, needle.as_str()])
        }
        ("GET", [index, "_search"]) => {
            let (verb, needle) = classify_search(&[], req.query.as_deref());
            dispatch(session, &verb, &[index, needle.as_str()])
        }
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
    if let Some(q) = query {
        for part in q.split('&') {
            if let Some((k, v)) = part.split_once('=') {
                if k == "q" {
                    return ("SEARCH_MATCH".into(), v.to_string());
                }
            }
        }
    }

    let text = String::from_utf8_lossy(body);
    if text.contains("\"term\"") {
        if let Ok(v) = serde_json::from_slice::<Value>(body) {
            if let Some(n) = extract_term_needle(&v) {
                return ("SEARCH_TERM".into(), n);
            }
        }
        return ("SEARCH_TERM".into(), String::new());
    }
    if text.contains("\"match\"") || text.contains("match") {
        if let Ok(v) = serde_json::from_slice::<Value>(body) {
            if let Some(n) = extract_match_needle(&v) {
                return ("SEARCH_MATCH".into(), n);
            }
        }
        return ("SEARCH_MATCH".into(), String::new());
    }
    ("SEARCH_MATCH".into(), String::new())
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
}

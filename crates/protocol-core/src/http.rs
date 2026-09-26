//! Minimal HTTP/1.x request-line helpers shared by ES/S3/WebDAV/CH-HTTP.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequestLine {
    pub method: String,
    pub target: String,
    pub version: String,
}

/// Parse `METHOD SP target SP HTTP/1.x` (no CRLF required in `line`).
pub fn parse_http_request_line(line: &str) -> Option<HttpRequestLine> {
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    let version = parts.next()?.to_string();
    if !version.starts_with("HTTP/1.") {
        return None;
    }
    Some(HttpRequestLine {
        method,
        target,
        version,
    })
}

/// Split path and raw query (`/a?b=c` → (`/a`, Some(`b=c`))).
pub fn split_target(target: &str) -> (&str, Option<&str>) {
    match target.split_once('?') {
        Some((path, q)) => (path, Some(q)),
        None => (target, None),
    }
}

//! Minimal sync HTTP helper for CLI → admin-http.

use std::io::{Read, Write};
use std::net::TcpStream;

pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

pub fn request(
    method: &str,
    endpoint: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&str>,
) -> Result<HttpResponse, String> {
    let endpoint = endpoint.trim_end_matches('/');
    let url = format!("{endpoint}{path}");
    let (host, port, path_q) = parse_http_url(&url)?;
    let mut stream = TcpStream::connect(format!("{host}:{port}"))
        .map_err(|e| format!("connect {host}:{port}: {e}"))?;
    let payload = body.unwrap_or("");
    let mut req = format!(
        "{method} {path_q} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nAccept: application/json\r\n"
    );
    if let Some(t) = token.filter(|s| !s.is_empty()) {
        req.push_str(&format!("Authorization: Bearer {t}\r\n"));
    }
    if !payload.is_empty() {
        req.push_str("Content-Type: application/json\r\n");
        req.push_str(&format!("Content-Length: {}\r\n", payload.len()));
    }
    req.push_str("\r\n");
    req.push_str(payload);
    stream
        .write_all(req.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    let mut buf = String::new();
    stream
        .read_to_string(&mut buf)
        .map_err(|e| format!("read: {e}"))?;
    let (status, body) = split_http(&buf)?;
    Ok(HttpResponse { status, body })
}

fn parse_http_url(url: &str) -> Result<(String, u16, String), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("only http:// supported: {url}"))?;
    let (hostport, path) = match rest.split_once('/') {
        Some((h, p)) => (h, format!("/{p}")),
        None => (rest, "/".into()),
    };
    let (host, port) = if let Some((h, p)) = hostport.split_once(':') {
        (
            h.to_string(),
            p.parse::<u16>().map_err(|e| format!("bad port: {e}"))?,
        )
    } else {
        (hostport.to_string(), 80)
    };
    Ok((host, port, path))
}

fn split_http(raw: &str) -> Result<(u16, String), String> {
    let (head, body) = raw
        .split_once("\r\n\r\n")
        .or_else(|| raw.split_once("\n\n"))
        .ok_or_else(|| "malformed HTTP response".to_string())?;
    let status_line = head.lines().next().unwrap_or("");
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Ok((status, body.to_string()))
}

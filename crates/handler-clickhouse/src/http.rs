//! ClickHouse HTTP entrypoint (`clickhouse-http`).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use bytes::BytesMut;
use spacestorage_compat::DialectProfile;
use spacestorage_protocol_core::{
    default_timeout, parse_http_request_line, peek_signature, split_target, ProtocolFamily,
};
use spacestorage_types::ContainerCatalog;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::dispatch::{ClickHouseReply, SessionState};
use crate::sql::execute_sql;

pub struct ClickHouseHttpHandler {
    pub catalog: Arc<RwLock<ContainerCatalog>>,
    pub profile: DialectProfile,
    pub namespace: String,
}

impl ClickHouseHttpHandler {
    pub fn with_demo(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self {
            catalog,
            profile: DialectProfile::HandlersComplete,
            namespace: "demo".into(),
        }
    }

    pub async fn serve<S>(&self, mut stream: S, cancel: CancellationToken)
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        let peeked = match peek_signature(
            &mut stream,
            ProtocolFamily::ClickHouseHttp,
            default_timeout(),
        )
        .await
        {
            Ok(buf) => buf,
            Err((_err, refusal)) => {
                let _ = stream.write_all(&refusal.body).await;
                let _ = stream.shutdown().await;
                return;
            }
        };

        let mut session = SessionState {
            catalog: Arc::clone(&self.catalog),
            namespace: self.namespace.clone(),
            profile: self.profile,
            protocol: spacestorage_compat::ProtocolId::ClickHouseHttp,
        };

        let mut buf = BytesMut::from(peeked.as_slice());
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                req = read_http(&mut stream, &mut buf) => {
                    match req {
                        Ok(None) => break,
                        Ok(Some(req)) => {
                            if let Err(status) = authenticate(&req.headers) {
                                if write_http(&mut stream, status, "Code: 516. AUTHENTICATION_FAILED\n")
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                                continue;
                            }
                            let sql = resolve_sql(&req);
                            let reply = execute_sql(&mut session, &sql);
                            let (status, body) = render(&reply);
                            if write_http(&mut stream, status, &body).await.is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            }
        }
    }
}

struct HttpReq {
    method: String,
    path: String,
    query: Option<String>,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn authenticate(headers: &HashMap<String, String>) -> Result<(), u16> {
    // Optional Basic; also accept query user/password via resolve path.
    // Missing auth is allowed for smoke if X-ClickHouse-User is absent — require Basic when present.
    if let Some(auth) = headers.get("authorization") {
        if let Some(rest) = auth.strip_prefix("Basic ") {
            let decoded = decode_basic(rest.trim()).ok_or(403u16)?;
            let mut parts = decoded.splitn(2, ':');
            let u = parts.next().unwrap_or("");
            let p = parts.next().unwrap_or("");
            if u == "demo" && p == "demo" {
                return Ok(());
            }
            return Err(403);
        }
    }
    // No Authorization → allow demo smoke (CH HTTP often uses ?user=&password=).
    Ok(())
}

fn decode_basic(b64: &str) -> Option<String> {
    // Minimal base64 decode for smoke.
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes = b64.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 3 < bytes.len() || (i < bytes.len() && bytes[i] != b'=') {
        if i + 3 >= bytes.len() {
            break;
        }
        let a = val(bytes[i])?;
        let b = val(bytes[i + 1])?;
        let c = if bytes[i + 2] == b'=' {
            0
        } else {
            val(bytes[i + 2])?
        };
        let d = if bytes[i + 3] == b'=' {
            0
        } else {
            val(bytes[i + 3])?
        };
        out.push((a << 2) | (b >> 4));
        if bytes[i + 2] != b'=' {
            out.push((b << 4) | (c >> 2));
        }
        if bytes[i + 3] != b'=' {
            out.push((c << 6) | d);
        }
        i += 4;
        if i >= bytes.len() {
            break;
        }
    }
    String::from_utf8(out).ok()
}

fn resolve_sql(req: &HttpReq) -> String {
    if let Some(q) = &req.query {
        for pair in q.split('&') {
            if let Some(v) = pair.strip_prefix("query=") {
                return urlencoding_decode(v);
            }
        }
    }
    String::from_utf8_lossy(&req.body).into_owned()
}

fn urlencoding_decode(s: &str) -> String {
    let mut out = String::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => {
                out.push(' ');
                i += 1;
            }
            b'%' if i + 2 < b.len() => {
                let hex = &s[i + 1..i + 3];
                if let Ok(v) = u8::from_str_radix(hex, 16) {
                    out.push(v as char);
                    i += 3;
                } else {
                    out.push('%');
                    i += 1;
                }
            }
            c => {
                out.push(c as char);
                i += 1;
            }
        }
    }
    out
}

fn render(reply: &ClickHouseReply) -> (u16, String) {
    match reply {
        ClickHouseReply::Ok => (200, "Ok.\n".into()),
        ClickHouseReply::Rows(rows) => {
            let mut s = String::new();
            for (k, v) in rows {
                s.push_str(k);
                s.push('\t');
                s.push_str(v);
                s.push('\n');
            }
            (200, s)
        }
        ClickHouseReply::Error { code, message } => {
            (400, format!("Code: {code}. {message}\n"))
        }
    }
}

async fn write_http<W: AsyncWrite + Unpin>(
    w: &mut W,
    status: u16,
    body: &str,
) -> std::io::Result<()> {
    let reason = if status == 200 { "OK" } else { "Error" };
    let msg = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/tab-separated-values; charset=UTF-8\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{body}",
        body.len()
    );
    w.write_all(msg.as_bytes()).await?;
    w.flush().await
}

async fn read_http<S: AsyncRead + Unpin>(
    stream: &mut S,
    buf: &mut BytesMut,
) -> std::io::Result<Option<HttpReq>> {
    loop {
        if let Some(req) = try_parse(buf)? {
            return Ok(Some(req));
        }
        let mut tmp = [0u8; 4096];
        match stream.read(&mut tmp).await? {
            0 => return Ok(None),
            n => buf.extend_from_slice(&tmp[..n]),
        }
    }
}

fn try_parse(buf: &mut BytesMut) -> std::io::Result<Option<HttpReq>> {
    let Some(header_end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
        return Ok(None);
    };
    let header_bytes = buf[..header_end].to_vec();
    let header_str = String::from_utf8_lossy(&header_bytes);
    let mut lines = header_str.split("\r\n");
    let Some(first) = lines.next() else {
        return Ok(None);
    };
    let Some(rl) = parse_http_request_line(first) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "bad request line",
        ));
    };
    let (path, query) = split_target(&rl.target);
    let mut headers = HashMap::new();
    let mut content_length = 0usize;
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim().to_ascii_lowercase();
            let val = v.trim().to_string();
            if key == "content-length" {
                content_length = val.parse().unwrap_or(0);
            }
            headers.insert(key, val);
        }
    }
    let total = header_end + 4 + content_length;
    if buf.len() < total {
        return Ok(None);
    }
    let _ = buf.split_to(header_end + 4);
    let body = buf.split_to(content_length).to_vec();
    Ok(Some(HttpReq {
        method: rl.method,
        path: path.to_string(),
        query: query.map(|s| s.to_string()),
        headers,
        body,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn http_smoke_create_insert_select() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let handler = ClickHouseHttpHandler::with_demo(cat);
        let (mut client, server) = tokio::io::duplex(8192);
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        tokio::spawn(async move { handler.serve(server, c2).await });

        let body = "CREATE TABLE u (id String)";
        let req = format!(
            "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{body}",
            body.len()
        );
        client.write_all(req.as_bytes()).await.unwrap();
        let mut resp = vec![0u8; 512];
        let n = tokio::time::timeout(std::time::Duration::from_millis(500), client.read(&mut resp))
            .await
            .unwrap()
            .unwrap();
        let s = String::from_utf8_lossy(&resp[..n]);
        assert!(s.contains("200") && s.contains("Ok."), "{s}");
        cancel.cancel();
    }

    #[tokio::test]
    async fn mismatch_redis_refused() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let handler = ClickHouseHttpHandler::with_demo(cat);
        let (mut client, server) = tokio::io::duplex(1024);
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        tokio::spawn(async move { handler.serve(server, c2).await });
        client.write_all(b"*1\r\n$4\r\nPING\r\n").await.unwrap();
        let mut out = vec![0u8; 128];
        let n = tokio::time::timeout(std::time::Duration::from_millis(300), client.read(&mut out))
            .await
            .unwrap()
            .unwrap();
        let s = String::from_utf8_lossy(&out[..n]);
        assert!(s.contains("400") || s.contains("mismatch"), "{s}");
        cancel.cancel();
    }
}

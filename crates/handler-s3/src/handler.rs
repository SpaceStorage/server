//! HTTP/1.1 path-style S3 wire (`/{bucket}/{key}`) with SigV4 auth.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use bytes::BytesMut;
use spacestorage_compat::DialectProfile;
use spacestorage_protocol_core::{
    default_timeout, parse_http_request_line, peek_signature, split_target, ProtocolFamily,
};
use spacestorage_types::ContainerCatalog;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::dispatch::{dispatch, put_object_bytes, upload_part_bytes, S3Reply, SessionState};
use crate::sigv4::{payload_hash_from_headers, verify_sigv4, SigV4Error, SigV4Request};

pub struct S3Handler {
    pub catalog: Arc<RwLock<ContainerCatalog>>,
    pub profile: DialectProfile,
    pub namespace: String,
}

impl S3Handler {
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
        let peeked = match peek_signature(&mut stream, ProtocolFamily::S3, default_timeout()).await {
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
            multipart: Default::default(),
        };

        let mut buf = BytesMut::from(peeked.as_slice());

        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                req = read_http_request(&mut stream, &mut buf) => {
                    match req {
                        Ok(None) => break,
                        Ok(Some(parsed)) => {
                            let request_id = next_request_id();
                            let reply = handle_http(&mut session, &parsed);
                            if write_http_response(&mut stream, &reply, &request_id).await.is_err() {
                                break;
                            }
                            if !parsed.keep_alive {
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

#[derive(Debug)]
struct ParsedHttpRequest {
    method: String,
    path: String,
    query: Option<String>,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    keep_alive: bool,
}

fn next_request_id() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(1);
    format!("{:016x}", SEQ.fetch_add(1, Ordering::Relaxed))
}

async fn read_http_request<S: AsyncRead + Unpin>(
    stream: &mut S,
    buf: &mut BytesMut,
) -> std::io::Result<Option<ParsedHttpRequest>> {
    loop {
        if let Some(req) = try_parse_http(buf)? {
            return Ok(Some(req));
        }
        let mut tmp = [0u8; 4096];
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&tmp[..n]);
    }
}

fn try_parse_http(buf: &mut BytesMut) -> std::io::Result<Option<ParsedHttpRequest>> {
    let Some(header_end) = find_header_end(buf) else {
        return Ok(None);
    };
    let header_bytes = buf[..header_end].to_vec();
    let headers_str = std::str::from_utf8(&header_bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let mut lines = headers_str.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let Some(line) = parse_http_request_line(request_line) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "bad request line",
        ));
    };
    let (path, query) = split_target(&line.target);
    let mut headers = Vec::new();
    for h in lines {
        if h.is_empty() {
            continue;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let content_length = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    let total = header_end + 4 + content_length;
    if buf.len() < total {
        return Ok(None);
    }
    let body = buf[header_end + 4..total].to_vec();
    buf.advance(total);
    let keep_alive = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("connection"))
        .map(|(_, v)| v.eq_ignore_ascii_case("keep-alive"))
        .unwrap_or(false);
    Ok(Some(ParsedHttpRequest {
        method: line.method,
        path: path.to_string(),
        query: query.map(str::to_string),
        headers,
        body,
        keep_alive,
    }))
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

trait BytesAdvance {
    fn advance(&mut self, n: usize);
}

impl BytesAdvance for BytesMut {
    fn advance(&mut self, n: usize) {
        let _ = self.split_to(n);
    }
}

fn header_lookup<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn header_pairs(headers: &[(String, String)]) -> Vec<(&str, &str)> {
    headers
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect()
}

/// Smoke-only bypass: `x-spacestorage-demo-auth: demo` skips SigV4 (real verify in `sigv4`).
fn authenticate(req: &ParsedHttpRequest) -> Result<(), (String, String)> {
    if header_lookup(&req.headers, "x-spacestorage-demo-auth") == Some("demo") {
        return Ok(());
    }
    let authorization = header_lookup(&req.headers, "authorization")
        .ok_or(("AccessDenied".into(), "Missing Authorization".into()))?;
    let amz_date = header_lookup(&req.headers, "x-amz-date")
        .ok_or(("AccessDenied".into(), "Missing x-amz-date".into()))?;
    let payload_hash = payload_hash_from_headers(&header_pairs(&req.headers))
        .map_err(|_| ("AccessDenied".into(), "Missing x-amz-content-sha256".into()))?;
    let sig_req = SigV4Request {
        method: &req.method,
        uri_path: &req.path,
        query: req.query.as_deref(),
        headers: &header_pairs(&req.headers),
        payload_hash: &payload_hash,
    };
    if let Err(e) = verify_sigv4(&sig_req, authorization, amz_date) {
        let (code, msg) = match e {
            SigV4Error::BadSignature | SigV4Error::BadAuthorization => (
                "SignatureDoesNotMatch".into(),
                "The request signature does not match".into(),
            ),
            SigV4Error::UnknownAccessKey => {
                ("InvalidAccessKeyId".into(), "Unknown access key".into())
            }
            _ => ("AccessDenied".into(), "Auth failed".into()),
        };
        return Err((code, msg));
    }
    Ok(())
}

fn parse_query(q: &str) -> HashMap<String, String> {
    q.split('&')
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            Some((k.to_string(), v.to_string()))
        })
        .collect()
}

fn split_bucket_key(path: &str) -> (Option<String>, Option<String>) {
    let path = path.strip_prefix('/').unwrap_or(path);
    if path.is_empty() {
        return (None, None);
    }
    match path.split_once('/') {
        None => (Some(path.to_string()), None),
        Some((bucket, rest)) if rest.is_empty() => (Some(bucket.to_string()), None),
        Some((bucket, key)) => (Some(bucket.to_string()), Some(key.to_string())),
    }
}

fn handle_http(session: &mut SessionState, req: &ParsedHttpRequest) -> S3Reply {
    if let Err((code, message)) = authenticate(req) {
        return S3Reply::Error { code, message };
    }

    let (bucket, key) = split_bucket_key(&req.path);
    let query = req.query.as_deref().map(parse_query).unwrap_or_default();

    let method = req.method.to_ascii_uppercase();
    match method.as_str() {
        "PUT" if key.is_none() => {
            let b = bucket.as_deref().unwrap_or("bucket");
            dispatch(session, "CREATEBUCKET", &[b])
        }
        "PUT" if query.contains_key("uploadId") && query.contains_key("partNumber") => {
            let upload_id = query.get("uploadId").map(String::as_str).unwrap_or("");
            upload_part_bytes(session, upload_id, req.body.clone())
        }
        "PUT" => {
            let b = bucket.as_deref().unwrap_or("bucket");
            let k = key.as_deref().unwrap_or("key");
            put_object_bytes(session, b, k, req.body.clone())
        }
        "GET" if key.is_none() && query.get("list-type").map(String::as_str) == Some("2") => {
            let b = bucket.as_deref().unwrap_or("bucket");
            let prefix = query.get("prefix").map(String::as_str).unwrap_or("");
            dispatch(session, "LISTOBJECTSV2", &[b, prefix])
        }
        "GET" => {
            let b = bucket.as_deref().unwrap_or("bucket");
            let k = key.as_deref().unwrap_or("key");
            dispatch(session, "GETOBJECT", &[b, k])
        }
        "DELETE" => {
            let b = bucket.as_deref().unwrap_or("bucket");
            let k = key.as_deref().unwrap_or("key");
            dispatch(session, "DELETEOBJECT", &[b, k])
        }
        "POST" if query.contains_key("uploads") => {
            let b = bucket.as_deref().unwrap_or("bucket");
            let k = key.as_deref().unwrap_or("key");
            dispatch(session, "CREATEMULTIPARTUPLOAD", &[b, k])
        }
        "POST" if query.contains_key("uploadId") => {
            let upload_id = query.get("uploadId").map(String::as_str).unwrap_or("");
            dispatch(session, "COMPLETEMULTIPARTUPLOAD", &[upload_id])
        }
        _ => S3Reply::Error {
            code: "NotImplemented".into(),
            message: format!("{} not supported", req.method),
        },
    }
}

async fn write_http_response<W: AsyncWrite + Unpin>(
    w: &mut W,
    reply: &S3Reply,
    request_id: &str,
) -> std::io::Result<()> {
    let bytes = match reply {
        S3Reply::Ok => format!(
            "HTTP/1.1 200 OK\r\nx-amz-request-id: {request_id}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .into_bytes(),
        S3Reply::Xml(x) => format!(
            "HTTP/1.1 200 OK\r\nx-amz-request-id: {request_id}\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{x}",
            x.len()
        )
        .into_bytes(),
        S3Reply::Body(b) => {
            let head = format!(
                "HTTP/1.1 200 OK\r\nx-amz-request-id: {request_id}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                b.len()
            );
            let mut out = head.into_bytes();
            out.extend_from_slice(b);
            out
        }
        S3Reply::Error { code, message } => {
            xml_error_response(request_id, code, message, status_for_code(code)).into_bytes()
        }
    };
    w.write_all(&bytes).await?;
    w.flush().await
}

fn status_for_code(code: &str) -> u16 {
    match code {
        "NoSuchKey" | "NoSuchUpload" => 404,
        "AccessDenied" | "SignatureDoesNotMatch" | "InvalidAccessKeyId" => 403,
        "compat_must_not" => 501,
        _ => 400,
    }
}

fn xml_error_response(request_id: &str, code: &str, message: &str, status: u16) -> String {
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Error><Code>{code}</Code><Message>{message}</Message><RequestId>{request_id}</RequestId></Error>"
    );
    format!(
        "HTTP/1.1 {status} {reason}\r\nx-amz-request-id: {request_id}\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len(),
        reason = status_text(status)
    )
}

fn status_text(status: u16) -> &'static str {
    match status {
        403 => "Forbidden",
        404 => "Not Found",
        501 => "Not Implemented",
        _ => "Bad Request",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sigv4::{sign_request, SigV4Request, DEMO_ACCESS_KEY, DEMO_SECRET_KEY};
    use sha2::{Digest, Sha256};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn sha256_hex(data: &[u8]) -> String {
        hex::encode(Sha256::digest(data))
    }

    fn build_signed_put(path: &str, body: &[u8], host: &str, unsigned: bool) -> String {
        let amz_date = "20220301T120000Z";
        let payload_hash = if unsigned {
            "UNSIGNED-PAYLOAD".into()
        } else {
            sha256_hex(body)
        };
        let headers = [
            ("host", host),
            ("x-amz-date", amz_date),
            ("x-amz-content-sha256", payload_hash.as_str()),
        ];
        let req = SigV4Request {
            method: "PUT",
            uri_path: path,
            query: None,
            headers: &headers,
            payload_hash: &payload_hash,
        };
        let auth = sign_request(
            DEMO_SECRET_KEY,
            DEMO_ACCESS_KEY,
            &req,
            amz_date,
            "20220301",
            "us-east-1",
            "s3",
            &["host", "x-amz-date", "x-amz-content-sha256"],
        );
        format!(
            "PUT {path} HTTP/1.1\r\nHost: {host}\r\nx-amz-date: {amz_date}\r\nx-amz-content-sha256: {payload_hash}\r\nAuthorization: {auth}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
    }

    async fn spawn_s3_listener(
        handler: S3Handler,
        cancel: CancellationToken,
    ) -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    accept = listener.accept() => {
                        let Ok((sock, _)) = accept else { break };
                        let h = S3Handler {
                            catalog: Arc::clone(&handler.catalog),
                            profile: handler.profile,
                            namespace: handler.namespace.clone(),
                        };
                        let c = cancel.child_token();
                        tokio::spawn(async move { h.serve(sock, c).await });
                    }
                }
            }
        });
        addr
    }

    #[tokio::test]
    async fn http_smoke_create_put_get_demo_header() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let handler = S3Handler::with_demo(Arc::clone(&cat));
        let cancel = CancellationToken::new();
        let addr = spawn_s3_listener(handler, cancel.clone()).await;

        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let req = "PUT /smoke HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        stream.write_all(req.as_bytes()).await.unwrap();
        let mut resp = vec![0u8; 4096];
        let n = stream.read(&mut resp).await.unwrap();
        let text = String::from_utf8_lossy(&resp[..n]);
        assert!(text.contains("200 OK"), "{text}");

        let mut stream2 = tokio::net::TcpStream::connect(addr).await.unwrap();
        let body = b"hello";
        let req2 = format!(
            "PUT /smoke/obj HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream2.write_all(req2.as_bytes()).await.unwrap();
        stream2.write_all(body).await.unwrap();
        let mut resp2 = vec![0u8; 4096];
        let n2 = stream2.read(&mut resp2).await.unwrap();
        assert!(String::from_utf8_lossy(&resp2[..n2]).contains("200 OK"));

        let mut stream3 = tokio::net::TcpStream::connect(addr).await.unwrap();
        let req3 = "GET /smoke/obj HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n";
        stream3.write_all(req3.as_bytes()).await.unwrap();
        let mut resp3 = vec![0u8; 4096];
        let n3 = stream3.read(&mut resp3).await.unwrap();
        let text3 = String::from_utf8_lossy(&resp3[..n3]);
        assert!(text3.contains("200 OK"));
        assert!(text3.contains("hello"));

        cancel.cancel();
    }

    #[tokio::test]
    async fn http_smoke_sigv4_put_get() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let handler = S3Handler::with_demo(Arc::clone(&cat));
        let cancel = CancellationToken::new();
        let addr = spawn_s3_listener(handler, cancel.clone()).await;

        let host = format!("127.0.0.1:{}", addr.port());
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let req = build_signed_put("/sig/b", b"data", &host, true);
        stream.write_all(req.as_bytes()).await.unwrap();
        stream.write_all(b"data").await.unwrap();
        let mut resp = vec![0u8; 4096];
        let n = stream.read(&mut resp).await.unwrap();
        assert!(String::from_utf8_lossy(&resp[..n]).contains("200 OK"));
        cancel.cancel();
    }

    #[tokio::test]
    async fn mismatch_sends_400_and_closes() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let handler = S3Handler::with_demo(cat);
        let cancel = CancellationToken::new();
        let addr = spawn_s3_listener(handler, cancel.clone()).await;

        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream.write_all(b"*1\r\n$4\r\nPING\r\n").await.unwrap();
        let mut resp = vec![0u8; 512];
        let n = stream.read(&mut resp).await.unwrap();
        let text = String::from_utf8_lossy(&resp[..n]);
        assert!(text.contains("400 Bad Request"));
        assert!(text.contains("protocol mismatch"));
        cancel.cancel();
    }
}

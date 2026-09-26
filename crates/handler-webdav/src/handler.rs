//! Async WebDAV HTTP/1.1 serve with signature peek and Basic/Digest auth.

use std::collections::HashMap;
use std::io;
use std::sync::{Arc, RwLock};

use spacestorage_compat::DialectProfile;
use spacestorage_protocol_core::{default_timeout, peek_signature, ProtocolFamily};
use spacestorage_types::ContainerCatalog;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::auth::{authorize, new_nonce, unauthorized_http};
use crate::dispatch::{dispatch, SessionState, WebDavReply};

pub struct WebDavHandler {
    pub catalog: Arc<RwLock<ContainerCatalog>>,
    pub profile: DialectProfile,
    pub namespace: String,
}

impl WebDavHandler {
    pub fn with_demo(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self {
            catalog,
            profile: DialectProfile::HandlersComplete,
            namespace: "demo".into(),
        }
    }

    pub async fn serve<S>(&self, stream: S, cancel: CancellationToken)
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        let (mut reader, mut writer) = tokio::io::split(stream);

        let peeked = match peek_signature(&mut reader, ProtocolFamily::WebDav, default_timeout())
            .await
        {
            Ok(buf) => buf,
            Err((_, refusal)) => {
                let _ = writer.write_all(&refusal.body).await;
                let _ = writer.shutdown().await;
                return;
            }
        };

        let mut conn = Conn {
            pending: peeked,
            inner: reader,
        };
        let mut session = SessionState {
            catalog: Arc::clone(&self.catalog),
            namespace: self.namespace.clone(),
            collection: "dav".into(),
            profile: self.profile,
        };
        let mut digest_nonce = new_nonce();

        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                req = read_http_request(&mut conn) => {
                    match req {
                        Ok(None) => break,
                        Ok(Some(parsed)) => {
                            if !authorize(
                                parsed.headers.get("authorization").map(String::as_str),
                                &parsed.method,
                                &parsed.path,
                                &digest_nonce,
                            ) {
                                digest_nonce = new_nonce();
                                let msg = unauthorized_http(&digest_nonce);
                                if writer.write_all(msg.as_bytes()).await.is_err() {
                                    break;
                                }
                                if writer.flush().await.is_err() {
                                    break;
                                }
                                continue;
                            }

                            let reply = route_request(&mut session, &parsed);
                            if write_reply(&mut writer, &reply).await.is_err() {
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

struct Conn<R> {
    pending: Vec<u8>,
    inner: R,
}

struct HttpRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn route_request(session: &mut SessionState, req: &HttpRequest) -> WebDavReply {
    let body_str = String::from_utf8_lossy(&req.body);
    match req.method.to_ascii_uppercase().as_str() {
        "MKCOL" => dispatch(session, "MKCOL", &[req.path.as_str()]),
        "PUT" => dispatch(
            session,
            "PUT",
            &[req.path.as_str(), body_str.as_ref()],
        ),
        "GET" => dispatch(session, "GET", &[req.path.as_str()]),
        "DELETE" => dispatch(session, "DELETE", &[req.path.as_str()]),
        "PROPFIND" => dispatch(session, "PROPFIND", &[req.path.as_str()]),
        "MOVE" => {
            let dest = req
                .headers
                .get("destination")
                .map(|d| destination_path(d))
                .unwrap_or_else(|| "/".into());
            dispatch(session, "MOVE", &[req.path.as_str(), dest.as_str()])
        }
        "COPY" => {
            let dest = req
                .headers
                .get("destination")
                .map(|d| destination_path(d))
                .unwrap_or_else(|| "/".into());
            dispatch(session, "COPY", &[req.path.as_str(), dest.as_str()])
        }
        other => dispatch(session, other, &[req.path.as_str()]),
    }
}

fn destination_path(header: &str) -> String {
    let trimmed = header.trim();
    if let Some(path) = trimmed.strip_prefix("http://") {
        path.split_once('/')
            .map(|(_, rest)| format!("/{rest}"))
            .unwrap_or_else(|| "/".into())
    } else if let Some(path) = trimmed.strip_prefix("https://") {
        path.split_once('/')
            .map(|(_, rest)| format!("/{rest}"))
            .unwrap_or_else(|| "/".into())
    } else {
        trimmed.to_string()
    }
}

async fn read_http_request<R: AsyncRead + Unpin>(
    conn: &mut Conn<R>,
) -> io::Result<Option<HttpRequest>> {
    loop {
        if let Some(end) = find_header_end(&conn.pending) {
            let head = conn.pending[..end].to_vec();
            let parsed = parse_headers(&head)?;
            let content_len = parsed
                .headers
                .get("content-length")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);
            let total = end + content_len;
            while conn.pending.len() < total {
                if !conn.read_more().await? {
                    if conn.pending.len() >= end {
                        break;
                    }
                    return Ok(None);
                }
            }
            let body = conn.pending[end..total.min(conn.pending.len())].to_vec();
            conn.pending.drain(..total.min(conn.pending.len()));
            return Ok(Some(HttpRequest {
                method: parsed.method,
                path: parsed.path,
                headers: parsed.headers,
                body,
            }));
        }
        if !conn.read_more().await? {
            return Ok(None);
        }
        if conn.pending.len() > 1024 * 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request too large",
            ));
        }
    }
}

impl<R: AsyncRead + Unpin> Conn<R> {
    async fn read_more(&mut self) -> io::Result<bool> {
        let mut chunk = [0u8; 4096];
        let n = self.inner.read(&mut chunk).await?;
        if n == 0 {
            return Ok(false);
        }
        self.pending.extend_from_slice(&chunk[..n]);
        Ok(true)
    }
}

struct ParsedHead {
    method: String,
    path: String,
    headers: HashMap<String, String>,
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4)
}

fn parse_headers(raw: &[u8]) -> io::Result<ParsedHead> {
    let text =
        std::str::from_utf8(raw).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let mut lines = text.split("\r\n");
    let request_line = lines.next().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "missing request line")
    })?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing method"))?
        .to_string();
    let path = parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing path"))?
        .to_string();
    let _version = parts.next();

    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    Ok(ParsedHead {
        method,
        path,
        headers,
    })
}

async fn write_reply<W: AsyncWrite + Unpin>(w: &mut W, reply: &WebDavReply) -> io::Result<()> {
    match reply {
        WebDavReply::Ok => {
            w.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .await?;
        }
        WebDavReply::MultiStatus(xml) => {
            let head = format!(
                "HTTP/1.1 207 Multi-Status\r\nContent-Type: application/xml\r\nContent-Length: {}\r\n\r\n",
                xml.len()
            );
            w.write_all(head.as_bytes()).await?;
            w.write_all(xml.as_bytes()).await?;
        }
        WebDavReply::Body(b) => {
            let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", b.len());
            w.write_all(head.as_bytes()).await?;
            w.write_all(b).await?;
        }
        WebDavReply::Error { code, message } => {
            let (status, reason) = match code.as_str() {
                "not_found" => ("404", "Not Found"),
                "compat_must_not" => ("403", "Forbidden"),
                _ => ("500", "Internal Server Error"),
            };
            let body = format!("{code}: {message}");
            let msg = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            w.write_all(msg.as_bytes()).await?;
        }
    }
    w.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{digest_response, DEMO_PASS, DEMO_USER, REALM};
    use spacestorage_protocol_core::MismatchRefusal;
    use std::sync::RwLock;
    use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};

    fn http_request(method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
        let mut req = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\n");
        for (k, v) in headers {
            req.push_str(&format!("{k}: {v}\r\n"));
        }
        req.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
        let mut out = req.into_bytes();
        out.extend_from_slice(body);
        out
    }

    #[tokio::test]
    async fn serve_smoke_basic_auth() {
        let catalog = Arc::new(RwLock::new(ContainerCatalog::new()));
        let h = WebDavHandler::with_demo(Arc::clone(&catalog));
        let (mut client, server) = duplex(65536);
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        let join = tokio::spawn(async move {
            h.serve(server, c2).await;
        });

        client
            .write_all(&http_request(
                "MKCOL",
                "/dir",
                &[("Authorization", "Basic ZGVtbzpkZW1v")],
                b"",
            ))
            .await
            .unwrap();
        let mut buf = vec![0u8; 4096];
        let n = client.read(&mut buf).await.unwrap();
        assert!(n > 0);
        assert!(String::from_utf8_lossy(&buf[..n]).contains("200 OK"));

        client
            .write_all(&http_request(
                "PUT",
                "/dir/f",
                &[("Authorization", "Basic ZGVtbzpkZW1v")],
                b"hi",
            ))
            .await
            .unwrap();
        let n = client.read(&mut buf).await.unwrap();
        assert!(String::from_utf8_lossy(&buf[..n]).contains("200 OK"));

        client
            .write_all(&http_request(
                "GET",
                "/dir/f",
                &[("Authorization", "Basic ZGVtbzpkZW1v")],
                b"",
            ))
            .await
            .unwrap();
        let n = client.read(&mut buf).await.unwrap();
        let resp = String::from_utf8_lossy(&buf[..n]);
        assert!(resp.contains("200 OK"));
        assert!(resp.contains("hi"));

        client
            .write_all(&http_request(
                "PROPFIND",
                "/",
                &[("Authorization", "Basic ZGVtbzpkZW1v")],
                b"",
            ))
            .await
            .unwrap();
        let n = client.read(&mut buf).await.unwrap();
        assert!(String::from_utf8_lossy(&buf[..n]).contains("207 Multi-Status"));

        client
            .write_all(&http_request(
                "MOVE",
                "/dir/f",
                &[
                    ("Authorization", "Basic ZGVtbzpkZW1v"),
                    ("Destination", "/dir/g"),
                ],
                b"",
            ))
            .await
            .unwrap();
        let n = client.read(&mut buf).await.unwrap();
        assert!(String::from_utf8_lossy(&buf[..n]).contains("200 OK"));

        client
            .write_all(&http_request(
                "DELETE",
                "/dir/g",
                &[("Authorization", "Basic ZGVtbzpkZW1v")],
                b"",
            ))
            .await
            .unwrap();
        let n = client.read(&mut buf).await.unwrap();
        assert!(String::from_utf8_lossy(&buf[..n]).contains("200 OK"));

        cancel.cancel();
        drop(client);
        let _ = join.await;
    }

    #[tokio::test]
    async fn missing_auth_returns_401_with_both_schemes() {
        let catalog = Arc::new(RwLock::new(ContainerCatalog::new()));
        let h = WebDavHandler::with_demo(catalog);
        let (mut client, server) = duplex(8192);
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        tokio::spawn(async move {
            h.serve(server, c2).await;
        });

        client
            .write_all(&http_request("GET", "/", &[], b""))
            .await
            .unwrap();
        let mut buf = vec![0u8; 4096];
        let n = client.read(&mut buf).await.unwrap();
        let resp = String::from_utf8_lossy(&buf[..n]);
        assert!(resp.contains("401 Unauthorized"));
        assert!(resp.contains("WWW-Authenticate: Basic"));
        assert!(resp.contains("WWW-Authenticate: Digest"));

        cancel.cancel();
    }

    #[tokio::test]
    async fn digest_auth_after_challenge() {
        let catalog = Arc::new(RwLock::new(ContainerCatalog::new()));
        let h = WebDavHandler::with_demo(catalog);
        let (mut client, server) = duplex(8192);
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        tokio::spawn(async move {
            h.serve(server, c2).await;
        });

        client
            .write_all(&http_request("GET", "/x", &[], b""))
            .await
            .unwrap();
        let mut buf = vec![0u8; 4096];
        let n = client.read(&mut buf).await.unwrap();
        let resp = String::from_utf8_lossy(&buf[..n]);
        let nonce = resp
            .lines()
            .find(|l| l.contains("Digest") && l.contains("nonce="))
            .and_then(|l| l.split("nonce=\"").nth(1))
            .and_then(|s| s.split('"').next())
            .expect("nonce in challenge")
            .to_string();

        let nc = "00000001";
        let cnonce = "client";
        let response = digest_response(
            DEMO_USER,
            DEMO_PASS,
            REALM,
            "GET",
            "/x",
            &nonce,
            nc,
            cnonce,
            "auth",
        );
        let digest = format!(
            r#"Digest username="{DEMO_USER}", realm="{REALM}", nonce="{nonce}", uri="/x", response="{response}", qop=auth, nc={nc}, cnonce="{cnonce}""#
        );
        client
            .write_all(&http_request("GET", "/x", &[("Authorization", &digest)], b""))
            .await
            .unwrap();
        let n = client.read(&mut buf).await.unwrap();
        let resp = String::from_utf8_lossy(&buf[..n]);
        assert!(resp.contains("404") || resp.contains("200"));

        cancel.cancel();
    }

    #[tokio::test]
    async fn signature_mismatch_closes_with_refusal() {
        let catalog = Arc::new(RwLock::new(ContainerCatalog::new()));
        let h = WebDavHandler::with_demo(catalog);
        let (mut client, server) = duplex(8192);
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        tokio::spawn(async move {
            h.serve(server, c2).await;
        });

        client
            .write_all(b"\x04\x00\x00\x01\x05\x00\x00\x00\x00")
            .await
            .unwrap();
        let mut buf = vec![0u8; 512];
        let n = client.read(&mut buf).await.unwrap();
        let expected = MismatchRefusal::for_family(ProtocolFamily::WebDav);
        assert_eq!(&buf[..n], expected.body.as_slice());

        cancel.cancel();
    }
}

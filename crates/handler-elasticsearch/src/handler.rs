//! Elasticsearch HTTP/1.1 wire handler (Basic auth, protocol-core signature).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use spacestorage_compat::DialectProfile;
use spacestorage_protocol_core::{
    default_timeout, peek_signature, ProtocolFamily, MismatchRefusal,
};
use spacestorage_types::ContainerCatalog;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::dispatch::{EsReply, SessionState};
use crate::router::{self, HttpRequest, ParseNeed};

pub struct ElasticsearchHandler {
    pub catalog: Arc<RwLock<ContainerCatalog>>,
    pub profile: DialectProfile,
    pub namespace: String,
}

impl ElasticsearchHandler {
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

        let mut buf = match peek_signature(
            &mut reader,
            ProtocolFamily::Elasticsearch,
            default_timeout(),
        )
        .await
        {
            Ok(peeked) => peeked,
            Err((_err, refusal)) => {
                let _ = write_mismatch(&mut writer, &refusal).await;
                return;
            }
        };

        let mut session = SessionState {
            catalog: Arc::clone(&self.catalog),
            namespace: self.namespace.clone(),
            profile: self.profile,
        };

        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                read = read_http_request(&mut reader, &mut buf) => {
                    match read {
                        Ok(Some(req)) => {
                            let reply = match authenticate(&req.headers) {
                                Ok(ns) => {
                                    session.namespace = ns;
                                    router::route(&mut session, &req)
                                }
                                Err(r) => r,
                            };
                            let close = req
                                .headers
                                .get("connection")
                                .is_some_and(|c| c.eq_ignore_ascii_case("close"));
                            if write_http_response(&mut writer, &reply, !close).await.is_err() {
                                break;
                            }
                            if close {
                                break;
                            }
                        }
                        Ok(None) => break,
                        Err(_) => break,
                    }
                }
            }
        }
    }
}

async fn write_mismatch<W: AsyncWrite + Unpin>(
    w: &mut W,
    refusal: &MismatchRefusal,
) -> std::io::Result<()> {
    w.write_all(&refusal.body).await?;
    w.flush().await
}

async fn read_http_request<R: AsyncRead + Unpin>(
    reader: &mut R,
    buf: &mut Vec<u8>,
) -> std::io::Result<Option<HttpRequest>> {
    const MAX: usize = 2 * 1024 * 1024;
    loop {
        match HttpRequest::parse(buf) {
            Ok((req, consumed)) => {
                buf.drain(..consumed);
                return Ok(Some(req));
            }
            Err(ParseNeed::Invalid) => return Ok(None),
            Err(ParseNeed::Headers) | Err(ParseNeed::Body) => {
                if buf.len() >= MAX {
                    return Ok(None);
                }
                let mut chunk = [0u8; 4096];
                let n = reader.read(&mut chunk).await?;
                if n == 0 {
                    return if buf.is_empty() {
                        Ok(None)
                    } else {
                        Ok(None)
                    };
                }
                buf.extend_from_slice(&chunk[..n]);
            }
        }
    }
}

fn authenticate(headers: &HashMap<String, String>) -> Result<String, EsReply> {
    let auth = headers
        .get("authorization")
        .ok_or_else(security_unauthorized)?;
    let token = auth
        .strip_prefix("Basic ")
        .or_else(|| auth.strip_prefix("basic "))
        .ok_or_else(security_unauthorized)?;
    let decoded = base64_decode(token.trim()).map_err(|_| security_unauthorized())?;
    let creds =
        String::from_utf8(decoded).map_err(|_| security_unauthorized())?;
    let (user, secret) = creds.split_once(':').ok_or_else(security_unauthorized)?;
    if user == "demo" && secret == "demo" {
        Ok("demo".into())
    } else {
        Err(security_unauthorized())
    }
}

fn security_unauthorized() -> EsReply {
    EsReply::Error {
        status: 401,
        body: serde_json::json!({
            "error": {
                "type": "security_exception",
                "reason": "missing authentication credentials for REST request"
            },
            "status": 401
        }),
    }
}

async fn write_http_response<W: AsyncWrite + Unpin>(
    w: &mut W,
    reply: &EsReply,
    keep_alive: bool,
) -> std::io::Result<()> {
    let (status, body) = match reply {
        EsReply::Ok(v) => (200u16, v.clone()),
        EsReply::Error { status, body } => (*status, body.clone()),
    };
    let payload = serde_json::to_string(&body).unwrap_or_else(|_| "{}".into());
    let status_text = status_text(status);
    let mut msg = format!(
        "HTTP/1.1 {status} {status_text}\r\n\
         Content-Type: application/json\r\n\
         X-Elastic-Product: Elasticsearch\r\n\
         Content-Length: {}\r\n",
        payload.len()
    );
    if status == 401 {
        msg.push_str("WWW-Authenticate: Basic realm=\"SpaceStorage\"\r\n");
    }
    if keep_alive {
        msg.push_str("Connection: keep-alive\r\n");
    } else {
        msg.push_str("Connection: close\r\n");
    }
    msg.push_str("\r\n");
    msg.push_str(&payload);
    w.write_all(msg.as_bytes()).await?;
    w.flush().await
}

fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Error",
    }
}

fn base64_decode(s: &str) -> Result<Vec<u8>, ()> {
    fn val(c: u8) -> Result<u8, ()> {
        match c {
            b'A'..=b'Z' => Ok(c - b'A'),
            b'a'..=b'z' => Ok(c - b'a' + 26),
            b'0'..=b'9' => Ok(c - b'0' + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err(()),
        }
    }
    let bytes: Vec<u8> = s.bytes().filter(|b| *b != b'=' && !b.is_ascii_whitespace()).collect();
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        if chunk.len() < 2 {
            return Err(());
        }
        let a = val(chunk[0])? as u32;
        let b = val(chunk[1])? as u32;
        let c = if chunk.len() > 2 {
            val(chunk[2])? as u32
        } else {
            0
        };
        let d = if chunk.len() > 3 {
            val(chunk[3])? as u32
        } else {
            0
        };
        let n = (a << 18) | (b << 12) | (c << 6) | d;
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const AUTH: &str = "Authorization: Basic ZGVtbzpkZW1v\r\n";

    async fn read_response(client: &mut (impl AsyncRead + Unpin)) -> String {
        let mut buf = vec![0u8; 8192];
        let n = client.read(&mut buf).await.unwrap();
        String::from_utf8_lossy(&buf[..n]).into_owned()
    }

    #[tokio::test]
    async fn http_smoke_create_index_search() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let handler = ElasticsearchHandler::with_demo(cat);
        let (mut client, server) = tokio::io::duplex(65536);
        let cancel = CancellationToken::new();
        tokio::spawn(async move {
            handler.serve(server, cancel).await;
        });

        let create = format!(
            "PUT /docs HTTP/1.1\r\nHost: localhost\r\n{AUTH}Content-Length: 0\r\n\r\n"
        );
        client.write_all(create.as_bytes()).await.unwrap();
        let resp = read_response(&mut client).await;
        assert!(resp.contains("200 OK"));
        assert!(resp.contains("X-Elastic-Product: Elasticsearch"));
        assert!(resp.contains("\"acknowledged\":true"));

        let index = format!(
            "PUT /docs/_doc/1 HTTP/1.1\r\nHost: localhost\r\n{AUTH}Content-Length: 17\r\n\r\n{{\"title\":\"hello\"}}"
        );
        client.write_all(index.as_bytes()).await.unwrap();
        let resp = read_response(&mut client).await;
        assert!(resp.contains("\"result\":\"created\""));

        let search = format!(
            "POST /docs/_search HTTP/1.1\r\nHost: localhost\r\n{AUTH}Content-Length: 37\r\nConnection: close\r\n\r\n{{\"query\":{{\"match\":{{\"title\":\"hello\"}}}}}}"
        );
        client.write_all(search.as_bytes()).await.unwrap();
        let resp = read_response(&mut client).await;
        assert!(resp.contains("\"hits\""));
        assert!(resp.contains("hello"));
    }

    #[tokio::test]
    async fn get_root_and_unauthorized() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let handler = ElasticsearchHandler::with_demo(cat);
        let (mut client, server) = tokio::io::duplex(65536);
        let cancel = CancellationToken::new();
        tokio::spawn(async move {
            handler.serve(server, cancel).await;
        });

        let root = format!(
            "GET / HTTP/1.1\r\nHost: localhost\r\n{AUTH}Content-Length: 0\r\nConnection: close\r\n\r\n"
        );
        client.write_all(root.as_bytes()).await.unwrap();
        let resp = read_response(&mut client).await;
        assert!(resp.contains("8.15.0"));
        assert!(resp.contains("You Know, for Search"));

        let (mut client2, server2) = tokio::io::duplex(4096);
        let handler2 = ElasticsearchHandler::with_demo(Arc::new(RwLock::new(ContainerCatalog::new())));
        tokio::spawn(async move {
            handler2.serve(server2, CancellationToken::new()).await;
        });
        client2
            .write_all(b"GET / HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\n\r\n")
            .await
            .unwrap();
        let resp = read_response(&mut client2).await;
        assert!(resp.contains("401"));
        assert!(resp.contains("security_exception"));
        assert!(resp.contains("WWW-Authenticate: Basic realm=\"SpaceStorage\""));
    }

    #[tokio::test]
    async fn redis_bytes_refused_on_serve() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let handler = ElasticsearchHandler::with_demo(cat);
        let (mut client, server) = tokio::io::duplex(4096);
        tokio::spawn(async move {
            handler.serve(server, CancellationToken::new()).await;
        });

        client
            .write_all(b"*1\r\n$4\r\nPING\r\n")
            .await
            .unwrap();
        let resp = read_response(&mut client).await;
        assert!(resp.contains("400 Bad Request"));
        assert!(resp.contains("protocol mismatch"));
    }

    #[test]
    fn dispatch_api_still_exported() {
        use crate::dispatch::dispatch;
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(cat);
        assert!(matches!(
            dispatch(&mut s, "CREATE_INDEX", &["docs"]),
            EsReply::Ok(_)
        ));
    }
}

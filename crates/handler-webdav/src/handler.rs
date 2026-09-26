//! Async WebDAV line serve.

use std::sync::{Arc, RwLock};

use spacestorage_compat::DialectProfile;
use spacestorage_types::ContainerCatalog;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;

use crate::dispatch::{SessionState, WebDavReply, dispatch};

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
        let (reader, mut writer) = tokio::io::split(stream);
        let mut lines = BufReader::new(reader).lines();
        let mut session = SessionState {
            catalog: Arc::clone(&self.catalog),
            namespace: self.namespace.clone(),
            collection: "dav".into(),
            profile: self.profile,
        };

        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                line = lines.next_line() => {
                    match line {
                        Ok(Some(l)) => {
                            let parts: Vec<&str> = l.split_whitespace().collect();
                            if parts.is_empty() {
                                continue;
                            }
                            let reply = dispatch(&mut session, parts[0], &parts[1..]);
                            if write_reply(&mut writer, &reply).await.is_err() {
                                break;
                            }
                        }
                        Ok(None) | Err(_) => break,
                    }
                }
            }
        }
    }
}

async fn write_reply<W: AsyncWrite + Unpin>(
    w: &mut W,
    reply: &WebDavReply,
) -> std::io::Result<()> {
    let msg = match reply {
        WebDavReply::Ok => "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_string(),
        WebDavReply::MultiStatus(xml) => format!(
            "HTTP/1.1 207 Multi-Status\r\nContent-Type: application/xml\r\nContent-Length: {}\r\n\r\n{xml}",
            xml.len()
        ),
        WebDavReply::Body(b) => format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
            b.len(),
            String::from_utf8_lossy(b)
        ),
        WebDavReply::Error { code, message } => {
            let body = format!("{code}: {message}");
            format!(
                "HTTP/1.1 403 Forbidden\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
        }
    };
    w.write_all(msg.as_bytes()).await?;
    w.flush().await
}

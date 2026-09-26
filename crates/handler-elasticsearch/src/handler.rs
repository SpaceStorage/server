//! Minimal HTTP-ish line serve for Elasticsearch handler.

use std::sync::{Arc, RwLock};

use spacestorage_compat::DialectProfile;
use spacestorage_types::ContainerCatalog;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;

use crate::dispatch::{EsReply, SessionState, dispatch};

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
        let (reader, mut writer) = tokio::io::split(stream);
        let mut lines = BufReader::new(reader).lines();
        let mut session = SessionState {
            catalog: Arc::clone(&self.catalog),
            namespace: self.namespace.clone(),
            profile: self.profile,
        };

        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                line = lines.next_line() => {
                    match line {
                        Ok(Some(l)) => {
                            let reply = handle_line(&mut session, &l);
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

fn handle_line(session: &mut SessionState, line: &str) -> EsReply {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.is_empty() {
        return EsReply::Ok(serde_json::json!({ "ok": true }));
    }
    dispatch(session, parts[0], &parts[1..])
}

async fn write_reply<W: AsyncWrite + Unpin>(w: &mut W, reply: &EsReply) -> std::io::Result<()> {
    let (status, body) = match reply {
        EsReply::Ok(v) => (200u16, v.clone()),
        EsReply::Error { status, body } => (*status, body.clone()),
    };
    let payload = serde_json::to_string(&body).unwrap_or_else(|_| "{}".into());
    let msg = format!("HTTP/1.1 {status} X\r\nContent-Length: {}\r\n\r\n{payload}\n", payload.len());
    w.write_all(msg.as_bytes()).await?;
    w.flush().await
}

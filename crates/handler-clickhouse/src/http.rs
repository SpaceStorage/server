//! ClickHouse HTTP entrypoint serve (`clickhouse-http`).

use std::sync::{Arc, RwLock};

use spacestorage_compat::DialectProfile;
use spacestorage_types::ContainerCatalog;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;

use crate::dispatch::{ClickHouseReply, SessionState, dispatch};

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
            protocol: spacestorage_compat::ProtocolId::ClickHouseHttp,
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
    reply: &ClickHouseReply,
) -> std::io::Result<()> {
    let (status, body) = match reply {
        ClickHouseReply::Ok => (200u16, "Ok.\n".to_string()),
        ClickHouseReply::Rows(rows) => {
            let mut s = String::new();
            for (k, v) in rows {
                s.push_str(&format!("{k}\t{v}\n"));
            }
            (200, s)
        }
        ClickHouseReply::Error { code, message } => {
            (400, format!("Code: {code}. {message}\n"))
        }
    };
    let msg = format!(
        "HTTP/1.1 {status} X\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    w.write_all(msg.as_bytes()).await?;
    w.flush().await
}

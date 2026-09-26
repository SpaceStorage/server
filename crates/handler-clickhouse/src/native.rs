//! ClickHouse native entrypoint serve.

use std::sync::{Arc, RwLock};

use spacestorage_compat::DialectProfile;
use spacestorage_types::ContainerCatalog;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;

use crate::dispatch::{ClickHouseReply, SessionState, dispatch};

pub struct ClickHouseNativeHandler {
    pub catalog: Arc<RwLock<ContainerCatalog>>,
    pub profile: DialectProfile,
    pub namespace: String,
}

impl ClickHouseNativeHandler {
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
            protocol: spacestorage_compat::ProtocolId::ClickHouse,
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
    let msg = match reply {
        ClickHouseReply::Ok => "Ok.\n".to_string(),
        ClickHouseReply::Rows(rows) => {
            let mut s = String::new();
            for (k, v) in rows {
                s.push_str(&format!("{k}\t{v}\n"));
            }
            s
        }
        ClickHouseReply::Error { code, message } => format!("Code: {code}. {message}\n"),
    };
    w.write_all(msg.as_bytes()).await?;
    w.flush().await
}

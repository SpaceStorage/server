//! Async line-oriented CQL serve loop (minimal native-v4 dialect stub).

use std::sync::{Arc, RwLock};

use spacestorage_compat::DialectProfile;
use spacestorage_types::ContainerCatalog;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;

use crate::dispatch::{CassandraReply, SessionState, dispatch};

pub struct CassandraHandler {
    pub catalog: Arc<RwLock<ContainerCatalog>>,
    pub profile: DialectProfile,
    pub namespace: String,
}

impl CassandraHandler {
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

fn handle_line(session: &mut SessionState, line: &str) -> CassandraReply {
    let line = line.trim();
    if line.is_empty() {
        return CassandraReply::Ok;
    }
    // Very small parser: VERB arg1 arg2 …
    let parts: Vec<&str> = line.split_whitespace().collect();
    let verb = parts[0];
    let args = &parts[1..];
    dispatch(session, verb, args)
}

async fn write_reply<W: AsyncWrite + Unpin>(
    w: &mut W,
    reply: &CassandraReply,
) -> std::io::Result<()> {
    let msg = match reply {
        CassandraReply::Ok => "OK\n".to_string(),
        CassandraReply::Prepared(id) => format!("PREPARED {id}\n"),
        CassandraReply::Rows(rows) => {
            let mut s = String::from("ROWS\n");
            for (k, v) in rows {
                s.push_str(&format!("{k}\t{v}\n"));
            }
            s.push_str("END\n");
            s
        }
        CassandraReply::Error { code, message } => format!("ERROR {code} {message}\n"),
    };
    w.write_all(msg.as_bytes()).await?;
    w.flush().await
}

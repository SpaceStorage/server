//! Async RESP2 serve loop (Constitution II).

use std::sync::{Arc, RwLock};

use bytes::BytesMut;
use spacestorage_types::ContainerCatalog;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use tracing::debug;

use crate::commands::{RedisReply, SessionState, dispatch};
use crate::resp::{RespError, RespValue, encode, try_decode};

pub struct RedisHandler {
    pub catalog: Arc<RwLock<ContainerCatalog>>,
    pub expected_user: String,
    pub expected_secret: String,
    pub namespace: String,
    pub default_container: String,
}

impl RedisHandler {
    pub fn with_demo(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self {
            catalog,
            expected_user: "demo".into(),
            expected_secret: "demo".into(),
            namespace: "demo".into(),
            default_container: "redis".into(),
        }
    }

    pub async fn serve<S>(&self, mut stream: S, cancel: CancellationToken)
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        let peeked = match spacestorage_protocol_core::peek_signature(
            &mut stream,
            spacestorage_protocol_core::ProtocolFamily::Redis,
            spacestorage_protocol_core::default_timeout(),
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
            ttls: Arc::new(RwLock::new(Default::default())),
            authenticated: false,
            namespace: self.namespace.clone(),
            container: self.default_container.clone(),
            password_ok: false,
            expected_secret: self.expected_secret.clone(),
            expected_user: self.expected_user.clone(),
        };

        let mut buf = BytesMut::from(peeked.as_slice());
        let mut read_buf = vec![0u8; 4096];

        loop {
            // Drain peeked / buffered frames before blocking on read.
            loop {
                match try_decode(&mut buf) {
                    Ok(None) => break,
                    Ok(Some(frame)) => {
                        let reply = handle_frame(&mut session, frame);
                        if write_reply(&mut stream, &reply).await.is_err() {
                            return;
                        }
                    }
                    Err(RespError::Incomplete) => break,
                    Err(e) => {
                        debug!(error = %e, "resp decode error");
                        let _ = write_reply(
                            &mut stream,
                            &RedisReply::Error(format!("ERR Protocol error: {e}")),
                        )
                        .await;
                        return;
                    }
                }
            }

            tokio::select! {
                _ = cancel.cancelled() => break,
                n = stream.read(&mut read_buf) => {
                    match n {
                        Ok(0) | Err(_) => break,
                        Ok(n) => buf.extend_from_slice(&read_buf[..n]),
                    }
                }
            }
        }
    }
}

fn handle_frame(session: &mut SessionState, frame: RespValue) -> RedisReply {
    let args = match frame_to_args(frame) {
        Ok(a) => a,
        Err(e) => return RedisReply::Error(e),
    };
    if args.is_empty() {
        return RedisReply::Error("ERR empty command".into());
    }
    let cmd = args[0].clone();
    let rest: Vec<&str> = args[1..].iter().map(|s| s.as_str()).collect();
    dispatch(session, &cmd, &rest)
}

fn frame_to_args(frame: RespValue) -> Result<Vec<String>, String> {
    match frame {
        RespValue::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for it in items {
                out.push(frame_to_string(it)?);
            }
            Ok(out)
        }
        other => Ok(vec![frame_to_string(other)?]),
    }
}

fn frame_to_string(frame: RespValue) -> Result<String, String> {
    match frame {
        RespValue::BulkString(Some(b)) => String::from_utf8(b).map_err(|e| e.to_string()),
        RespValue::BulkString(None) => Ok(String::new()),
        RespValue::SimpleString(s) => Ok(s),
        RespValue::Integer(i) => Ok(i.to_string()),
        RespValue::Error(e) => Ok(e),
        RespValue::Array(_) => Err("ERR unexpected nested array".into()),
    }
}

async fn write_reply<S: AsyncWrite + Unpin>(
    stream: &mut S,
    reply: &RedisReply,
) -> std::io::Result<()> {
    let mut out = BytesMut::new();
    encode(&reply_to_value(reply), &mut out);
    stream.write_all(&out).await?;
    stream.flush().await
}

fn reply_to_value(reply: &RedisReply) -> RespValue {
    match reply {
        RedisReply::Ok => RespValue::SimpleString("OK".into()),
        RedisReply::Pong => RespValue::SimpleString("PONG".into()),
        RedisReply::Bulk(None) => RespValue::BulkString(None),
        RedisReply::Bulk(Some(b)) => RespValue::BulkString(Some(b.clone())),
        RedisReply::Integer(i) => RespValue::Integer(*i),
        RedisReply::Array(items) => RespValue::Array(items.iter().map(reply_to_value).collect()),
        RedisReply::Error(e) => RespValue::Error(e.clone()),
    }
}

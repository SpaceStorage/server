use crate::admin::auth::check_bearer;
use crate::admin::AdminService;
use crate::Node;
use async_trait::async_trait;
use bytes::BytesMut;
use spacestorage_admin_proto::{decode_frame, encode_frame, AdminOp, ErrorBody};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;
use tracing::debug;

use super::Handler;

pub struct AdminTcpHandler {
    pub node: Arc<Node>,
}

#[async_trait]
impl Handler for AdminTcpHandler {
    fn name(&self) -> &str {
        "admin"
    }

    fn kind(&self) -> &str {
        "admin"
    }

    async fn serve(&self, mut stream: TcpStream, cancel: CancellationToken) {
        let mut buf = BytesMut::with_capacity(4096);
        // Hello timeout 5s
        let hello = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if buf.len() > 4 * 1024 * 1024 {
                    return Err("too large");
                }
                let n = stream.read_buf(&mut buf).await.map_err(|_| "read")?;
                if n == 0 {
                    return Err("eof");
                }
                if let Ok(Some(payload)) = decode_frame(&mut buf) {
                    return Ok(payload);
                }
            }
        })
        .await;

        let Ok(Ok(hello_bytes)) = hello else {
            return;
        };
        let hello_json: serde_json::Value = match serde_json::from_slice(&hello_bytes) {
            Ok(v) => v,
            Err(_) => return,
        };
        let token = hello_json
            .get("token")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let cfg = self.node.config.load();
        let expected = cfg
            .admin_token_file
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_default();
        if !check_bearer(token, expected.trim()) {
            let err = ErrorBody {
                code: "unauthorized".into(),
                message: "invalid token".into(),
                details: serde_json::json!({}),
            };
            if let Ok(body) = serde_json::to_vec(&err) {
                if let Ok(frame) = encode_frame(&body) {
                    let _ = stream.write_all(&frame).await;
                }
            }
            return;
        }
        // ack hello
        let ack = serde_json::json!({"ok": true, "version": 1});
        if let Ok(body) = serde_json::to_vec(&ack) {
            if let Ok(frame) = encode_frame(&body) {
                let _ = stream.write_all(&frame).await;
            }
        }

        let idle = Duration::from_secs(600);
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                r = tokio::time::timeout(idle, stream.read_buf(&mut buf)) => {
                    match r {
                        Ok(Ok(0)) | Err(_) => break,
                        Ok(Err(_)) => break,
                        Ok(Ok(_)) => {
                            while let Ok(Some(payload)) = decode_frame(&mut buf) {
                                let resp = handle_op(&self.node, &payload).await;
                                if let Ok(frame) = encode_frame(&resp) {
                                    if stream.write_all(&frame).await.is_err() {
                                        return;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        debug!("admin tcp connection closed");
    }
}

async fn handle_op(node: &Arc<Node>, payload: &[u8]) -> Vec<u8> {
    let op: AdminOp = match serde_json::from_slice(payload) {
        Ok(o) => o,
        Err(_) => {
            return serde_json::to_vec(&ErrorBody {
                code: "unknown_op".into(),
                message: "cannot parse op".into(),
                details: serde_json::json!({}),
            })
            .unwrap_or_default();
        }
    };
    match AdminService::execute(node, op).await {
        Ok(v) => serde_json::to_vec(&v).unwrap_or_default(),
        Err(e) => serde_json::to_vec(&e).unwrap_or_default(),
    }
}

//! Real internode / replication accept loops (012) replacing stub_cluster.

use crate::handler::ClientStream;
use async_trait::async_trait;
use spacestorage_internode::{
    decode_frame, encode_frame, registry, FabricRuntime, CURRENT_VERSION,
};
use spacestorage_membership::{JoinRequest, MembershipService};
use spacestorage_replication::DurableAccept;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use tracing::debug;

use super::Handler;

pub struct InternodeHandler {
    pub fabric: Arc<FabricRuntime>,
    pub membership: Arc<tokio::sync::RwLock<Option<Arc<MembershipService>>>>,
}

pub struct ReplicationHandler {
    pub fabric: Arc<FabricRuntime>,
    pub durable: Arc<DurableAccept>,
}

#[async_trait]
impl Handler for InternodeHandler {
    fn name(&self) -> &str {
        "internode"
    }

    fn kind(&self) -> &str {
        "cluster"
    }

    fn owner(&self) -> &str {
        "012-internode-and-time"
    }

    async fn serve(&self, stream: ClientStream, cancel: CancellationToken) {
        serve_framed(
            stream,
            cancel,
            &self.fabric,
            Some(Arc::clone(&self.membership)),
            None,
        )
        .await;
    }
}

#[async_trait]
impl Handler for ReplicationHandler {
    fn name(&self) -> &str {
        "replication"
    }

    fn kind(&self) -> &str {
        "cluster"
    }

    fn owner(&self) -> &str {
        "012-internode-and-time"
    }

    async fn serve(&self, stream: ClientStream, cancel: CancellationToken) {
        serve_framed(
            stream,
            cancel,
            &self.fabric,
            None,
            Some(Arc::clone(&self.durable)),
        )
        .await;
    }
}

async fn serve_framed(
    mut stream: ClientStream,
    cancel: CancellationToken,
    fabric: &FabricRuntime,
    membership: Option<Arc<tokio::sync::RwLock<Option<Arc<MembershipService>>>>>,
    durable: Option<Arc<DurableAccept>>,
) {
    // First frame MUST be auth: msg_type 0 with secret bytes as payload.
    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 4096];
    let mut authed = false;
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            r = stream.read(&mut tmp) => {
                match r {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&tmp[..n]);
                        while let Ok((frame, consumed)) = decode_frame(&buf) {
                            buf.drain(..consumed);
                            if !authed {
                                if !fabric.verify_presented_secret(&frame.payload) {
                                    debug!("internode auth refused");
                                    break;
                                }
                                authed = true;
                                let _ = stream.write_all(&encode_frame(registry::MSG_ACK, br#"{"ok":true}"#)).await;
                                continue;
                            }
                            if let Some(accept) = durable.as_ref() {
                                // T071: durable:true only after SourceLog::append_durable (spawn_blocking fsync).
                                match accept.ack_after_durable(frame.payload.clone()).await {
                                    Ok(ack) => {
                                        let _ = stream.write_all(&encode_frame(registry::MSG_ACK, &ack)).await;
                                    }
                                    Err(e) => {
                                        debug!(error = %e, "replication durable append failed");
                                        let err = format!(r#"{{"ok":false,"error":"{e}"}}"#);
                                        let _ = stream.write_all(&encode_frame(registry::MSG_ACK, err.as_bytes())).await;
                                    }
                                }
                            } else {
                                handle_control(
                                    fabric,
                                    membership.as_ref(),
                                    &mut stream,
                                    frame.msg_type,
                                    &frame.payload,
                                )
                                .await;
                            }
                            let _ = CURRENT_VERSION;
                        }
                        if !authed && buf.len() > 1 << 20 {
                            break;
                        }
                    }
                }
            }
        }
        if !authed && cancel.is_cancelled() {
            break;
        }
    }
}

async fn handle_control(
    fabric: &FabricRuntime,
    membership: Option<&Arc<tokio::sync::RwLock<Option<Arc<MembershipService>>>>>,
    stream: &mut ClientStream,
    msg_type: u16,
    payload: &[u8],
) {
    if msg_type == registry::MSG_HEARTBEAT {
        if let Ok(peer) = std::str::from_utf8(payload) {
            fabric.fd.note_heartbeat(peer.trim());
        }
        let _ = stream
            .write_all(&encode_frame(registry::MSG_ACK, br#"{"ok":true}"#))
            .await;
        return;
    }
    if msg_type == registry::MSG_JOIN_REQUEST {
        let ack_bytes = match handle_join_request(membership, payload).await {
            Ok(bytes) => bytes,
            Err(e) => {
                debug!(error = %e, "join request failed");
                serde_json::to_vec(&spacestorage_membership::JoinAck::Refused {
                    code: e,
                })
                .unwrap_or_else(|_| br#"{"status":"refused","code":"internal"}"#.to_vec())
            }
        };
        let _ = stream
            .write_all(&encode_frame(registry::MSG_JOIN_ACK, &ack_bytes))
            .await;
        return;
    }
    if !registry::is_known(msg_type) {
        let _ = stream
            .write_all(&encode_frame(
                registry::MSG_ACK,
                registry::unknown_ack_payload(),
            ))
            .await;
        return;
    }
    let _ = stream
        .write_all(&encode_frame(registry::MSG_ACK, br#"{"ok":true}"#))
        .await;
}

async fn handle_join_request(
    membership: Option<&Arc<tokio::sync::RwLock<Option<Arc<MembershipService>>>>>,
    payload: &[u8],
) -> Result<Vec<u8>, String> {
    let Some(slot) = membership else {
        return Err("membership_unavailable".into());
    };
    let guard = slot.read().await;
    let Some(svc) = guard.as_ref() else {
        return Err("membership_unavailable".into());
    };
    let req: JoinRequest =
        serde_json::from_slice(payload).map_err(|e| format!("join_decode:{e}"))?;
    let ack = svc.apply_join(req).map_err(|e| e.to_string())?;
    serde_json::to_vec(&ack).map_err(|e| e.to_string())
}

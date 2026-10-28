//! Best-effort quota delta fanout (007 R4).

use crate::{encode_frame, registry, FabricRuntime, QuotaDelta};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Fire-and-forget publish to internode peers (does not wait for cluster primary).
pub async fn broadcast_quota_delta(
    fabric: &FabricRuntime,
    peer_addrs: &[String],
    delta: QuotaDelta,
) {
    let payload = match serde_json::to_vec(&delta) {
        Ok(p) => p,
        Err(_) => return,
    };
    let secret = fabric.join_secret_bytes();
    for addr in peer_addrs {
        let _ = send_delta_once(addr, &secret, &payload).await;
    }
}

async fn send_delta_once(addr: &str, secret: &[u8], payload: &[u8]) -> Result<(), ()> {
    let mut stream = TcpStream::connect(addr).await.map_err(|_| ())?;
    stream
        .write_all(&encode_frame(0, secret))
        .await
        .map_err(|_| ())?;
    let mut buf = vec![0u8; 4096];
    let _ = stream.read(&mut buf).await;
    stream
        .write_all(&encode_frame(registry::MSG_QUOTA_DELTA, payload))
        .await
        .map_err(|_| ())?;
    Ok(())
}

pub fn spawn_broadcast(
    fabric: Arc<FabricRuntime>,
    peer_addrs: Vec<String>,
    delta: QuotaDelta,
) {
    tokio::spawn(async move {
        broadcast_quota_delta(fabric.as_ref(), &peer_addrs, delta).await;
    });
}

//! First-join seed contact: JoinRequest → JoinAck over internode framing.

use crate::error::{MembershipError, Result};
use crate::join::{JoinAck, JoinRequest, SeedEndpoint};
use spacestorage_internode::{
    decode_frame, encode_frame, registry, CURRENT_VERSION,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use std::time::Duration;
use tracing::{debug, info, warn};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const IO_TIMEOUT: Duration = Duration::from_secs(10);

/// Contact seeds in order until one returns a usable `JoinAck`.
pub async fn contact_seeds(
    seeds: &[SeedEndpoint],
    secret: &[u8],
    req: &JoinRequest,
) -> Result<JoinAck> {
    if seeds.is_empty() {
        return Err(MembershipError::InvalidState("no_seeds".into()));
    }
    let payload = serde_json::to_vec(req)
        .map_err(|e| MembershipError::Io(e.to_string()))?;
    let mut last_err = MembershipError::InvalidState("no_seed_reachable".into());
    for seed in seeds {
        let addr = seed.host_port();
        match contact_one(&addr, secret, &payload).await {
            Ok(ack) => {
                info!(seed = %addr, ack = ?ack_status(&ack), "membership: seed JoinAck");
                return Ok(ack);
            }
            Err(e) => {
                warn!(seed = %addr, error = %e, "membership: seed contact failed");
                last_err = e;
            }
        }
    }
    Err(last_err)
}

fn ack_status(ack: &JoinAck) -> &'static str {
    match ack {
        JoinAck::Pending => "pending",
        JoinAck::Admitted { .. } => "admitted",
        JoinAck::Replaced { .. } => "replaced",
        JoinAck::Refused { .. } => "refused",
    }
}

async fn contact_one(addr: &str, secret: &[u8], join_payload: &[u8]) -> Result<JoinAck> {
    let mut stream = timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
        .await
        .map_err(|_| MembershipError::Io(format!("connect_timeout:{addr}")))?
        .map_err(|e| MembershipError::Io(e.to_string()))?;

    // Auth frame: msg_type 0, payload = join secret bytes.
    write_frame(&mut stream, 0, secret).await?;
    let auth_ack = read_frame(&mut stream).await?;
    if auth_ack.msg_type != registry::MSG_ACK {
        return Err(MembershipError::Io("auth_ack_type".into()));
    }
    let _ = CURRENT_VERSION;

    write_frame(&mut stream, registry::MSG_JOIN_REQUEST, join_payload).await?;
    let resp = read_frame(&mut stream).await?;
    if resp.msg_type != registry::MSG_JOIN_ACK && resp.msg_type != registry::MSG_ACK {
        return Err(MembershipError::Io(format!(
            "unexpected_msg_type:{}",
            resp.msg_type
        )));
    }
    let ack: JoinAck = serde_json::from_slice(&resp.payload)
        .map_err(|e| MembershipError::Io(format!("join_ack_decode:{e}")))?;
    debug!(addr, "membership: decoded JoinAck");
    Ok(ack)
}

async fn write_frame(stream: &mut TcpStream, msg_type: u16, payload: &[u8]) -> Result<()> {
    let enc = encode_frame(msg_type, payload);
    timeout(IO_TIMEOUT, stream.write_all(&enc))
        .await
        .map_err(|_| MembershipError::Io("write_timeout".into()))?
        .map_err(|e| MembershipError::Io(e.to_string()))
}

async fn read_frame(stream: &mut TcpStream) -> Result<spacestorage_internode::Frame> {
    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 4096];
    let deadline = tokio::time::Instant::now() + IO_TIMEOUT;
    loop {
        if tokio::time::Instant::now() > deadline {
            return Err(MembershipError::Io("read_timeout".into()));
        }
        match decode_frame(&buf) {
            Ok((frame, n)) => {
                buf.drain(..n);
                return Ok(frame);
            }
            Err(spacestorage_internode::FrameError::Truncated) => {}
            Err(e) => return Err(MembershipError::Io(e.to_string())),
        }
        let n = timeout(deadline.saturating_duration_since(tokio::time::Instant::now()), stream.read(&mut tmp))
            .await
            .map_err(|_| MembershipError::Io("read_timeout".into()))?
            .map_err(|e| MembershipError::Io(e.to_string()))?;
        if n == 0 {
            return Err(MembershipError::Io("eof".into()));
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > 1 << 20 {
            return Err(MembershipError::Io("frame_too_large".into()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::join::JoinAck;
    use spacestorage_internode::{encode_frame, registry};
    use std::collections::BTreeMap;
    use tokio::net::TcpListener;
    use uuid::Uuid;

    #[tokio::test]
    async fn contact_seed_returns_pending_ack() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let secret = b"test-secret-bytes-0123456789ab".to_vec();
        let secret_accept = secret.clone();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = Vec::new();
            let mut tmp = [0u8; 2048];
            // auth
            let n = sock.read(&mut tmp).await.unwrap();
            buf.extend_from_slice(&tmp[..n]);
            let (frame, _) = decode_frame(&buf).unwrap();
            assert_eq!(frame.payload, secret_accept);
            sock.write_all(&encode_frame(registry::MSG_ACK, br#"{"ok":true}"#))
                .await
                .unwrap();
            buf.clear();
            let n = sock.read(&mut tmp).await.unwrap();
            buf.extend_from_slice(&tmp[..n]);
            let (frame, _) = decode_frame(&buf).unwrap();
            assert_eq!(frame.msg_type, registry::MSG_JOIN_REQUEST);
            let ack = serde_json::to_vec(&JoinAck::Pending).unwrap();
            sock.write_all(&encode_frame(registry::MSG_JOIN_ACK, &ack))
                .await
                .unwrap();
        });

        let seeds = [SeedEndpoint {
            name: "db-1".into(),
            address: addr.ip().to_string(),
            port: addr.port(),
        }];
        let req = JoinRequest {
            node_id: Uuid::new_v4(),
            node_name: "n2".into(),
            labels: BTreeMap::from([("az".into(), "a".into())]),
            internodes_address: "127.0.0.1:1".into(),
            quorum_domain: "lab".into(),
            presented_secret: secret.clone(),
            join_token: None,
            replace_of: None,
            product_version: 1,
        };
        let ack = contact_seeds(&seeds, &secret, &req).await.unwrap();
        assert_eq!(ack, JoinAck::Pending);
    }
}

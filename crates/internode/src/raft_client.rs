//! Dial `BasicNode.addr` and exchange Raft peer RPCs over live internodes TCP.
//!
//! Protocol (same as membership seed contact):
//! 1. Auth frame `msg_type=0` with join secret → `MSG_ACK`
//! 2. Raft request frame (`MSG_RAFT_VOTE` / `APPEND` / `SNAPSHOT`) with JSON body
//! 3. Response `MSG_ACK` whose payload is the openraft JSON response (or error object)

use crate::frame::{decode_frame, encode_frame, Frame, FrameError};
use crate::raft_wire::{RaftRpcKind, RaftWireError};
use crate::registry;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const IO_TIMEOUT: Duration = Duration::from_secs(10);

/// Normalize `BasicNode.addr` into a TCP dial target (`host:port`).
///
/// Accepts bare `127.0.0.1:7000`, `tcp://127.0.0.1:7000`, or `host:port`.
/// Returns `None` for in-process / non-TCP schemes (`inproc://…`).
pub fn tcp_dial_addr(addr: &str) -> Option<String> {
    let a = addr.trim();
    if a.is_empty() || a.starts_with("inproc://") || a.starts_with("memory://") {
        return None;
    }
    let a = a
        .strip_prefix("tcp://")
        .or_else(|| a.strip_prefix("internode://"))
        .unwrap_or(a);
    if a.contains(':') {
        Some(a.to_string())
    } else {
        None
    }
}

/// Dial `addr`, authenticate with `secret`, send one Raft RPC, return response JSON bytes.
pub async fn raft_rpc(
    addr: &str,
    secret: &[u8],
    kind: RaftRpcKind,
    body: &[u8],
) -> Result<Vec<u8>, RaftWireError> {
    let dial = tcp_dial_addr(addr).ok_or_else(|| {
        RaftWireError::Frame(format!("raft_client: not a tcp addr: {addr}"))
    })?;
    let mut stream = timeout(CONNECT_TIMEOUT, TcpStream::connect(&dial))
        .await
        .map_err(|_| RaftWireError::Frame(format!("connect_timeout:{dial}")))?
        .map_err(|e| RaftWireError::Frame(e.to_string()))?;

    write_frame(&mut stream, 0, secret).await?;
    let auth_ack = read_frame(&mut stream).await?;
    if auth_ack.msg_type != registry::MSG_ACK {
        return Err(RaftWireError::UnexpectedMsg(auth_ack.msg_type));
    }

    write_frame(&mut stream, kind.msg_type(), body).await?;
    let resp = read_frame(&mut stream).await?;
    if resp.msg_type != registry::MSG_ACK {
        return Err(RaftWireError::UnexpectedMsg(resp.msg_type));
    }
    // Error envelope from fabric: {"ok":false,"error":"…"}
    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&resp.payload) {
        if v.get("ok") == Some(&serde_json::Value::Bool(false)) {
            let err = v
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("raft_rpc_failed");
            return Err(RaftWireError::Frame(err.to_string()));
        }
    }
    Ok(resp.payload)
}

async fn write_frame(stream: &mut TcpStream, msg_type: u16, payload: &[u8]) -> Result<(), RaftWireError> {
    let enc = encode_frame(msg_type, payload);
    timeout(IO_TIMEOUT, stream.write_all(&enc))
        .await
        .map_err(|_| RaftWireError::Frame("write_timeout".into()))?
        .map_err(|e| RaftWireError::Frame(e.to_string()))
}

async fn read_frame(stream: &mut TcpStream) -> Result<Frame, RaftWireError> {
    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 4096];
    let deadline = tokio::time::Instant::now() + IO_TIMEOUT;
    loop {
        if tokio::time::Instant::now() > deadline {
            return Err(RaftWireError::Frame("read_timeout".into()));
        }
        match decode_frame(&buf) {
            Ok((frame, n)) => {
                buf.drain(..n);
                return Ok(frame);
            }
            Err(FrameError::Truncated) => {}
            Err(e) => return Err(RaftWireError::Frame(e.to_string())),
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let n = timeout(remaining, stream.read(&mut tmp))
            .await
            .map_err(|_| RaftWireError::Frame("read_timeout".into()))?
            .map_err(|e| RaftWireError::Frame(e.to_string()))?;
        if n == 0 {
            return Err(RaftWireError::Frame("eof".into()));
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > 1 << 20 {
            return Err(RaftWireError::Frame("frame_too_large".into()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raft_wire::encode_raft_payload;
    use tokio::net::TcpListener;

    #[test]
    fn tcp_dial_addr_parses() {
        assert_eq!(
            tcp_dial_addr("127.0.0.1:7000").as_deref(),
            Some("127.0.0.1:7000")
        );
        assert_eq!(
            tcp_dial_addr("tcp://127.0.0.1:7000").as_deref(),
            Some("127.0.0.1:7000")
        );
        assert!(tcp_dial_addr("inproc://abc").is_none());
        assert!(tcp_dial_addr("").is_none());
    }

    #[tokio::test]
    async fn raft_rpc_roundtrip_over_tcp() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let secret = b"raft-test-secret-0123456789abcd".to_vec();
        let secret_accept = secret.clone();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
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
            assert_eq!(frame.msg_type, registry::MSG_RAFT_VOTE);
            sock.write_all(&encode_frame(
                registry::MSG_ACK,
                br#"{"vote_granted":true}"#,
            ))
            .await
            .unwrap();
        });

        let body = br#"{"term":1}"#;
        let dial = format!("{}:{}", addr.ip(), addr.port());
        let resp = raft_rpc(&dial, &secret, RaftRpcKind::Vote, body)
            .await
            .unwrap();
        assert!(resp.windows(b"vote_granted".len()).any(|w| w == b"vote_granted"));
        let _ = encode_raft_payload(RaftRpcKind::Vote, &serde_json::json!({"term":1}));
    }
}

//! Raft peer RPC framing over internodes (006 contracts/raft-rpc.md).
//!
//! Payload is length-prefixed frame body (JSON). Message types:
//! [`registry::MSG_RAFT_VOTE`], [`registry::MSG_RAFT_APPEND`], [`registry::MSG_RAFT_SNAPSHOT`].

use crate::frame::{decode_frame, encode_frame, FrameError};
use crate::registry::{MSG_RAFT_APPEND, MSG_RAFT_SNAPSHOT, MSG_RAFT_VOTE};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RaftWireError {
    #[error("raft_wire_decode: {0}")]
    Decode(String),
    #[error("raft_wire_encode: {0}")]
    Encode(String),
    #[error("raft_wire_frame: {0}")]
    Frame(String),
    #[error("raft_wire_unexpected_msg: {0}")]
    UnexpectedMsg(u16),
}

impl From<FrameError> for RaftWireError {
    fn from(e: FrameError) -> Self {
        Self::Frame(e.to_string())
    }
}

/// Discriminant for which openraft RPC is in the payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RaftRpcKind {
    Vote,
    Append,
    Snapshot,
}

impl RaftRpcKind {
    pub fn msg_type(self) -> u16 {
        match self {
            Self::Vote => MSG_RAFT_VOTE,
            Self::Append => MSG_RAFT_APPEND,
            Self::Snapshot => MSG_RAFT_SNAPSHOT,
        }
    }

    pub fn from_msg_type(t: u16) -> Option<Self> {
        match t {
            MSG_RAFT_VOTE => Some(Self::Vote),
            MSG_RAFT_APPEND => Some(Self::Append),
            MSG_RAFT_SNAPSHOT => Some(Self::Snapshot),
            _ => None,
        }
    }
}

/// Encode a typed Raft request/response body into an internode frame.
pub fn encode_raft_payload<T: Serialize>(kind: RaftRpcKind, body: &T) -> Result<Vec<u8>, RaftWireError> {
    let payload =
        serde_json::to_vec(body).map_err(|e| RaftWireError::Encode(e.to_string()))?;
    Ok(encode_frame(kind.msg_type(), &payload))
}

/// Decode a framed Raft RPC; returns kind + JSON body bytes.
pub fn decode_raft_frame(buf: &[u8]) -> Result<(RaftRpcKind, Vec<u8>), RaftWireError> {
    let (frame, _) = decode_frame(buf)?;
    let kind = RaftRpcKind::from_msg_type(frame.msg_type)
        .ok_or(RaftWireError::UnexpectedMsg(frame.msg_type))?;
    Ok((kind, frame.payload))
}

/// Decode JSON body into `T`.
pub fn decode_raft_body<T: DeserializeOwned>(payload: &[u8]) -> Result<T, RaftWireError> {
    serde_json::from_slice(payload).map_err(|e| RaftWireError::Decode(e.to_string()))
}

/// True when `msg_type` is a Raft peer RPC (vote / append / snapshot).
pub fn is_raft_peer_rpc(msg_type: u16) -> bool {
    RaftRpcKind::from_msg_type(msg_type).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn roundtrip_vote_frame() {
        let body = json!({"term": 1, "candidate": "a"});
        let frame = encode_raft_payload(RaftRpcKind::Vote, &body).unwrap();
        let (kind, payload) = decode_raft_frame(&frame).unwrap();
        assert_eq!(kind, RaftRpcKind::Vote);
        let back: serde_json::Value = decode_raft_body(&payload).unwrap();
        assert_eq!(back["term"], 1);
    }

    #[test]
    fn known_msg_types() {
        assert!(is_raft_peer_rpc(MSG_RAFT_VOTE));
        assert!(is_raft_peer_rpc(MSG_RAFT_APPEND));
        assert!(is_raft_peer_rpc(MSG_RAFT_SNAPSHOT));
        assert!(!is_raft_peer_rpc(1));
    }
}

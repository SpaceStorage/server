//! Client session state machine (FR-040).

use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Connecting,
    Handshaking,
    Ready,
    Executing,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionCloseReason {
    Mismatch,
    Auth,
    Drain,
    ClientEof,
    Error,
}

#[derive(Debug, Clone)]
pub struct ClientSession {
    pub id: u64,
    pub protocol: String,
    pub protocol_version: String,
    pub state: SessionState,
    pub close_reason: Option<SessionCloseReason>,
    pub namespace: Option<String>,
    pub started_at: Instant,
}

impl ClientSession {
    pub fn connecting(id: u64, protocol: impl Into<String>) -> Self {
        Self {
            id,
            protocol: protocol.into(),
            protocol_version: String::new(),
            state: SessionState::Connecting,
            close_reason: None,
            namespace: None,
            started_at: Instant::now(),
        }
    }

    pub fn advance_handshake(&mut self) {
        if self.state == SessionState::Connecting {
            self.state = SessionState::Handshaking;
        }
    }

    pub fn ready(&mut self, namespace: impl Into<String>, version: impl Into<String>) {
        self.namespace = Some(namespace.into());
        self.protocol_version = version.into();
        self.state = SessionState::Ready;
    }

    pub fn close(&mut self, reason: SessionCloseReason) {
        self.state = SessionState::Closed;
        self.close_reason = Some(reason);
    }
}

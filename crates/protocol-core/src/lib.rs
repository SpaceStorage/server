//! Shared protocol driver toolkit (feature `002`).
//!
//! Handlers depend on this crate for signature detection, session state, and
//! thin HTTP request-line helpers. Storage/engine crates MUST NOT depend here.

pub mod errors;
pub mod handshake;
pub mod http;
pub mod namespace;
pub mod options;
pub mod session;
pub mod signature;
pub mod stats;

pub use errors::{MismatchRefusal, ProtocolCoreError};
pub use handshake::{default_timeout, peek_signature};
pub use http::{parse_http_request_line, split_target, HttpRequestLine};
pub use session::{ClientSession, SessionCloseReason, SessionState};
pub use signature::{
    detect_signature, expect_signature, ProtocolFamily, SignatureMatch, DEFAULT_HANDSHAKE_TIMEOUT,
};

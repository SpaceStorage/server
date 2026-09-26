//! Async handshake peek with FR-004 deadline.

use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::time::{timeout, Instant};

use crate::errors::MismatchRefusal;
use crate::signature::{
    detect_signature, ProtocolFamily, SignatureError, SignatureMatch, DEFAULT_HANDSHAKE_TIMEOUT,
};

/// Read until signature decides match/mismatch or `handshake_timeout` elapses.
///
/// On success returns the peeked buffer (still owned by caller to re-parse).
/// On mismatch returns `(SignatureError, optional refusal bytes to write)`.
pub async fn peek_signature<R: AsyncRead + Unpin>(
    reader: &mut R,
    expected: ProtocolFamily,
    handshake_timeout: Duration,
) -> Result<Vec<u8>, (SignatureError, MismatchRefusal)> {
    let deadline = Instant::now() + handshake_timeout;
    let mut buf = Vec::with_capacity(256);
    let mut tmp = [0u8; 256];

    loop {
        match detect_signature(expected, &buf) {
            SignatureMatch::Ok { .. } => return Ok(buf),
            SignatureMatch::Mismatch { reason } => {
                return Err((
                    SignatureError::Mismatch {
                        handler: expected.as_str(),
                        reason,
                    },
                    MismatchRefusal::for_family(expected),
                ));
            }
            SignatureMatch::NeedMore => {}
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err((
                SignatureError::Timeout {
                    handler: expected.as_str(),
                },
                MismatchRefusal::for_family(expected),
            ));
        }

        match timeout(remaining, reader.read(&mut tmp)).await {
            Ok(Ok(0)) => {
                return Err((
                    SignatureError::Mismatch {
                        handler: expected.as_str(),
                        reason: "eof",
                    },
                    MismatchRefusal::for_family(expected),
                ));
            }
            Ok(Ok(n)) => buf.extend_from_slice(&tmp[..n]),
            Ok(Err(_)) => {
                return Err((
                    SignatureError::Mismatch {
                        handler: expected.as_str(),
                        reason: "io",
                    },
                    MismatchRefusal::for_family(expected),
                ));
            }
            Err(_) => {
                return Err((
                    SignatureError::Timeout {
                        handler: expected.as_str(),
                    },
                    MismatchRefusal::for_family(expected),
                ));
            }
        }
    }
}

pub fn default_timeout() -> Duration {
    DEFAULT_HANDSHAKE_TIMEOUT
}

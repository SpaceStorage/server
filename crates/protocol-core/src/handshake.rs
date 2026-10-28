//! Async handshake peek with FR-004 deadline.

use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::time::{Instant, timeout};

use crate::errors::MismatchRefusal;
use crate::signature::{
    DEFAULT_HANDSHAKE_TIMEOUT, ProtocolFamily, SignatureError, SignatureMatch, detect_signature,
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
                return refuse(
                    expected,
                    reason,
                    &buf,
                    SignatureError::Mismatch {
                        handler: expected.as_str(),
                        reason,
                    },
                );
            }
            SignatureMatch::NeedMore => {}
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return refuse(
                expected,
                "handshake_timeout",
                &buf,
                SignatureError::Timeout {
                    handler: expected.as_str(),
                },
            );
        }

        match timeout(remaining, reader.read(&mut tmp)).await {
            Ok(Ok(0)) => {
                return refuse(
                    expected,
                    "eof",
                    &buf,
                    SignatureError::Mismatch {
                        handler: expected.as_str(),
                        reason: "eof",
                    },
                );
            }
            Ok(Ok(n)) => buf.extend_from_slice(&tmp[..n]),
            Ok(Err(_)) => {
                return refuse(
                    expected,
                    "io",
                    &buf,
                    SignatureError::Mismatch {
                        handler: expected.as_str(),
                        reason: "io",
                    },
                );
            }
            Err(_) => {
                return refuse(
                    expected,
                    "handshake_timeout",
                    &buf,
                    SignatureError::Timeout {
                        handler: expected.as_str(),
                    },
                );
            }
        }
    }
}

pub fn default_timeout() -> Duration {
    DEFAULT_HANDSHAKE_TIMEOUT
}

/// FR-004: mismatch refusals are logged with the handler name. The node accept
/// loop enters a `client_conn` span carrying `entrypoint` before `Handler::serve`,
/// so the event includes both names.
fn refuse(
    expected: ProtocolFamily,
    reason: &'static str,
    buf: &[u8],
    err: SignatureError,
) -> Result<Vec<u8>, (SignatureError, MismatchRefusal)> {
    log_protocol_mismatch(expected.as_str(), reason, buf);
    Err((err, MismatchRefusal::for_family(expected)))
}

fn log_protocol_mismatch(handler: &str, reason: &str, buf: &[u8]) {
    tracing::warn!(
        handler,
        reason,
        first_bytes_hex = %hex::encode(buf),
        "protocol_mismatch"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::fmt::MakeWriter;

    struct SharedBuf(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedBuf {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("log buf").extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for SharedBuf {
        type Writer = SharedBuf;

        fn make_writer(&'a self) -> Self::Writer {
            SharedBuf(Arc::clone(&self.0))
        }
    }

    #[test]
    fn mismatch_log_names_handler_and_reason() {
        let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::WARN)
            .with_ansi(false)
            .with_writer(SharedBuf(Arc::clone(&buf)))
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            log_protocol_mismatch("postgresql", "bad_startup_code", b"GET /");
        });
        let text = String::from_utf8(buf.lock().expect("log buf").clone()).unwrap();
        assert!(text.contains("protocol_mismatch"), "log text: {text}");
        assert!(text.contains("postgresql"), "log text: {text}");
        assert!(text.contains("bad_startup_code"), "log text: {text}");
        assert!(text.contains("handler"), "log text: {text}");
    }

    #[tokio::test]
    async fn peek_mismatch_still_refuses() {
        let mut data = &b"GET / HTTP/1.1\r\n\r\n"[..];
        let err = peek_signature(
            &mut data,
            ProtocolFamily::Postgresql,
            Duration::from_secs(1),
        )
        .await
        .expect_err("http on postgresql must mismatch");
        assert!(matches!(err.0, SignatureError::Mismatch { .. }));
    }
}

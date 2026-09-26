//! First-bytes signature detection (FR-004 / FR-005).

use std::time::Duration;

use thiserror::Error;

/// Default handshake deadline (FR-004).
pub const DEFAULT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(1);

/// Protocol family expected on an entrypoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProtocolFamily {
    Postgresql,
    Redis,
    Cassandra,
    Elasticsearch,
    S3,
    WebDav,
    ClickHouseNative,
    ClickHouseHttp,
}

impl ProtocolFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Postgresql => "postgresql",
            Self::Redis => "redis",
            Self::Cassandra => "cassandra",
            Self::Elasticsearch => "elasticsearch",
            Self::S3 => "s3",
            Self::WebDav => "webdav",
            Self::ClickHouseNative => "clickhouse",
            Self::ClickHouseHttp => "clickhouse-http",
        }
    }

    /// True when this family speaks HTTP/1.x on the wire.
    pub fn is_http(self) -> bool {
        matches!(
            self,
            Self::Elasticsearch | Self::S3 | Self::WebDav | Self::ClickHouseHttp
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureMatch {
    /// Bytes match the expected protocol; `consumed` is 0 (peek) unless noted.
    Ok { peek_len: usize },
    /// Bytes clearly belong to a different known protocol or are garbage.
    Mismatch { reason: &'static str },
    /// Need more bytes before deciding (within the handshake deadline).
    NeedMore,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SignatureError {
    #[error("protocol_mismatch{{handler={handler}, reason={reason}}}")]
    Mismatch {
        handler: &'static str,
        reason: &'static str,
    },
    #[error("handshake_timeout{{handler={handler}}}")]
    Timeout { handler: &'static str },
}

/// Classify peeked buffer for `expected` without consuming.
pub fn detect_signature(expected: ProtocolFamily, buf: &[u8]) -> SignatureMatch {
    if buf.is_empty() {
        return SignatureMatch::NeedMore;
    }
    match expected {
        ProtocolFamily::Postgresql => detect_postgresql(buf),
        ProtocolFamily::Redis => detect_redis(buf),
        ProtocolFamily::Cassandra => detect_cassandra(buf),
        ProtocolFamily::ClickHouseNative => detect_clickhouse_native(buf),
        ProtocolFamily::Elasticsearch
        | ProtocolFamily::S3
        | ProtocolFamily::WebDav
        | ProtocolFamily::ClickHouseHttp => detect_http(buf),
    }
}

/// Convenience: Ok when match, Err on definitive mismatch. NeedMore stays Ok(false).
pub fn expect_signature(
    expected: ProtocolFamily,
    buf: &[u8],
) -> Result<Option<usize>, SignatureError> {
    match detect_signature(expected, buf) {
        SignatureMatch::Ok { peek_len } => Ok(Some(peek_len)),
        SignatureMatch::NeedMore => Ok(None),
        SignatureMatch::Mismatch { reason } => Err(SignatureError::Mismatch {
            handler: expected.as_str(),
            reason,
        }),
    }
}

fn detect_postgresql(buf: &[u8]) -> SignatureMatch {
    if buf.len() < 8 {
        return SignatureMatch::NeedMore;
    }
    let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let code = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
    // StartupMessage length 8..=10000; SSLRequest/Cancel/GSSENC codes.
    const SSL: u32 = 80877103;
    const CANCEL: u32 = 80877102;
    const GSS: u32 = 80877104;
    const V3: u32 = 196608;
    if !(8..=10_000).contains(&len) {
        return SignatureMatch::Mismatch {
            reason: "bad_startup_length",
        };
    }
    if code == V3 || code == SSL || code == CANCEL || code == GSS {
        SignatureMatch::Ok { peek_len: 8 }
    } else {
        SignatureMatch::Mismatch {
            reason: "bad_startup_code",
        }
    }
}

fn detect_redis(buf: &[u8]) -> SignatureMatch {
    let b0 = buf[0];
    // RESP array / simple / bulk / inline ASCII command.
    if b0 == b'*' || b0 == b'+' || b0 == b'$' || b0 == b':' || b0 == b'-' {
        return SignatureMatch::Ok { peek_len: 1 };
    }
    if b0.is_ascii_alphabetic() {
        // Reject HTTP request-lines misdirected to Redis (GET / HTTP/1.1 …).
        if buf.windows(7).any(|w| w.eq_ignore_ascii_case(b" HTTP/1")) {
            return SignatureMatch::Mismatch {
                reason: "http_not_resp",
            };
        }
        // Inline command; need CRLF or at least a few bytes.
        if buf.windows(2).any(|w| w == b"\r\n") || buf.len() >= 16 {
            return SignatureMatch::Ok { peek_len: 1 };
        }
        return SignatureMatch::NeedMore;
    }
    SignatureMatch::Mismatch {
        reason: "not_resp",
    }
}

fn detect_cassandra(buf: &[u8]) -> SignatureMatch {
    // Header: version | flags | stream(2) | opcode | length(4)
    if buf.len() < 9 {
        return SignatureMatch::NeedMore;
    }
    let version = buf[0];
    let major = version & 0x7f;
    let request = (version & 0x80) == 0;
    if !(request && (major == 4 || major == 5)) {
        return SignatureMatch::Mismatch {
            reason: "bad_cql_version",
        };
    }
    let opcode = buf[4];
    // OPTIONS=0x05, STARTUP=0x01 are the legal first opcodes.
    if opcode == 0x05 || opcode == 0x01 {
        SignatureMatch::Ok { peek_len: 9 }
    } else {
        SignatureMatch::Mismatch {
            reason: "bad_cql_opcode",
        }
    }
}

fn detect_clickhouse_native(buf: &[u8]) -> SignatureMatch {
    // First packet is Hello = varint 0, then length-prefixed client name.
    if buf[0] != 0 {
        return SignatureMatch::Mismatch {
            reason: "not_ch_hello",
        };
    }
    if buf.len() < 2 {
        return SignatureMatch::NeedMore;
    }
    // client name length as varuint (we accept single-byte lengths ≤ 127 for signature).
    let name_len = buf[1] as usize;
    if name_len > 127 {
        return SignatureMatch::Mismatch {
            reason: "ch_name_too_long",
        };
    }
    if buf.len() < 2 + name_len {
        return SignatureMatch::NeedMore;
    }
    SignatureMatch::Ok {
        peek_len: 2 + name_len,
    }
}

fn detect_http(buf: &[u8]) -> SignatureMatch {
    // HTTP/2 connection preface
    const H2: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
    if buf.len() >= 3 && &buf[..3] == b"PRI" {
        if buf.len() < H2.len() {
            return SignatureMatch::NeedMore;
        }
        if buf.starts_with(H2) {
            // h2c refused later by handlers; signature still "HTTP family".
            return SignatureMatch::Ok { peek_len: H2.len() };
        }
        return SignatureMatch::Mismatch {
            reason: "bad_h2_preface",
        };
    }
    // Request-line: METHOD SP …
    let methods = [
        "GET", "POST", "PUT", "DELETE", "HEAD", "OPTIONS", "PATCH", "PROPFIND", "PROPPATCH",
        "MKCOL", "COPY", "MOVE", "LOCK", "UNLOCK", "REPORT",
    ];
    let Some(line_end) = buf.windows(2).position(|w| w == b"\r\n") else {
        if buf.len() < 8 {
            return SignatureMatch::NeedMore;
        }
        // Enough bytes without CRLF → not HTTP.
        if !buf.iter().any(|b| b.is_ascii_uppercase() || *b == b' ') {
            return SignatureMatch::Mismatch {
                reason: "not_http",
            };
        }
        return SignatureMatch::NeedMore;
    };
    let line = std::str::from_utf8(&buf[..line_end]).unwrap_or("");
    let mut parts = line.split_whitespace();
    let Some(method) = parts.next() else {
        return SignatureMatch::Mismatch {
            reason: "empty_request_line",
        };
    };
    let Some(_path) = parts.next() else {
        return SignatureMatch::Mismatch {
            reason: "no_path",
        };
    };
    let Some(version) = parts.next() else {
        return SignatureMatch::Mismatch {
            reason: "no_version",
        };
    };
    if !version.starts_with("HTTP/1.") {
        return SignatureMatch::Mismatch {
            reason: "bad_http_version",
        };
    }
    if methods.iter().any(|m| method.eq_ignore_ascii_case(m)) {
        SignatureMatch::Ok {
            peek_len: line_end + 2,
        }
    } else {
        SignatureMatch::Mismatch {
            reason: "unknown_http_method",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redis_star() {
        assert!(matches!(
            detect_signature(ProtocolFamily::Redis, b"*1\r\n$4\r\nPING\r\n"),
            SignatureMatch::Ok { .. }
        ));
    }

    #[test]
    fn pg_ssl_request() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&8u32.to_be_bytes());
        buf.extend_from_slice(&80877103u32.to_be_bytes());
        assert!(matches!(
            detect_signature(ProtocolFamily::Postgresql, &buf),
            SignatureMatch::Ok { .. }
        ));
    }

    #[test]
    fn cassandra_options() {
        // v4 request OPTIONS
        let mut buf = vec![0x04, 0x00, 0x00, 0x01, 0x05];
        buf.extend_from_slice(&0u32.to_be_bytes());
        assert!(matches!(
            detect_signature(ProtocolFamily::Cassandra, &buf),
            SignatureMatch::Ok { .. }
        ));
    }

    #[test]
    fn http_get() {
        assert!(matches!(
            detect_signature(ProtocolFamily::Elasticsearch, b"GET / HTTP/1.1\r\n"),
            SignatureMatch::Ok { .. }
        ));
    }

    #[test]
    fn mismatch_redis_on_http() {
        assert!(matches!(
            detect_signature(ProtocolFamily::Redis, b"GET / HTTP/1.1\r\n"),
            SignatureMatch::Mismatch { .. }
        ));
    }

    #[test]
    fn clickhouse_hello() {
        // Hello=0, name_len=5, "click"
        let mut buf = vec![0u8, 5];
        buf.extend_from_slice(b"click");
        assert!(matches!(
            detect_signature(ProtocolFamily::ClickHouseNative, &buf),
            SignatureMatch::Ok { .. }
        ));
    }

    #[test]
    fn cross_protocol_matrix_under_deadline_shape() {
        // Wrong-protocol bytes must decide mismatch without needing a full timeout.
        let cases = [
            (ProtocolFamily::Cassandra, &b"GET / HTTP/1.1\r\nHost: x\r\n\r\n"[..]),
            (ProtocolFamily::S3, &b"*1\r\n$4\r\nPING\r\n"[..]),
            (ProtocolFamily::WebDav, &b"\x04\x00\x00\x01\x05\x00\x00\x00\x00"[..]),
            (ProtocolFamily::ClickHouseNative, &b"GET / HTTP/1.1\r\n\r\n"[..]),
        ];
        for (fam, bytes) in cases {
            assert!(
                matches!(detect_signature(fam, bytes), SignatureMatch::Mismatch { .. }),
                "{fam:?}"
            );
        }
    }
}

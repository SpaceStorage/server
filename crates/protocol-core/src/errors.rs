//! Protocol mismatch refusal payloads (data-model §8).

use crate::signature::ProtocolFamily;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProtocolCoreError {
    #[error("{0}")]
    Message(String),
}

/// Bytes to send before closing on signature mismatch (FR-004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MismatchRefusal {
    pub body: Vec<u8>,
}

impl MismatchRefusal {
    pub fn for_family(family: ProtocolFamily) -> Self {
        let body = match family {
            ProtocolFamily::Postgresql => {
                // ErrorResponse 08P01 — minimal; handlers may replace with full encoder.
                b"E".to_vec() // placeholder marker; PG handler writes full ErrorResponse
            }
            ProtocolFamily::Redis => b"-ERR unknown protocol\r\n".to_vec(),
            ProtocolFamily::Cassandra => {
                // ERROR PROTOCOL_ERROR (0x000A) — handlers encode full frame.
                b"PROTOCOL_ERROR".to_vec()
            }
            ProtocolFamily::ClickHouseNative => {
                b"Code: 101. UNEXPECTED_PACKET_FROM_CLIENT\n".to_vec()
            }
            ProtocolFamily::Elasticsearch
            | ProtocolFamily::S3
            | ProtocolFamily::WebDav
            | ProtocolFamily::ClickHouseHttp => {
                b"HTTP/1.1 400 Bad Request\r\nContent-Type: text/plain\r\nContent-Length: 16\r\nConnection: close\r\n\r\nprotocol mismatch"
                    .to_vec()
            }
        };
        Self { body }
    }
}

//! Streaming upload/download helpers — chunked SigV4 bodies decode before storage.
//! Catalog and durable quorum waits are `dispatch::exec` (`spawn_blocking`), not this module.

use tokio::io::{AsyncRead, AsyncReadExt};

/// One decoded aws-chunked body plus per-chunk signatures (empty when unsigned).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedChunks {
    pub data: Vec<u8>,
    pub chunk_signatures: Vec<(Vec<u8>, String)>,
}

/// Decode `aws-chunked` bodies (`STREAMING-AWS4-HMAC-SHA256-PAYLOAD` and unsigned trailer).
pub fn decode_aws_chunked(body: &[u8]) -> Result<DecodedChunks, String> {
    let mut data = Vec::new();
    let mut chunk_signatures = Vec::new();
    let mut i = 0;
    while i < body.len() {
        let line_end = body[i..]
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or_else(|| "truncated chunk header".to_string())?;
        let line = std::str::from_utf8(&body[i..i + line_end]).map_err(|e| e.to_string())?;
        i += line_end + 2;
        let (size_hex, sig) = match line.split_once(';') {
            Some((size, rest)) => {
                let sig = rest
                    .split(';')
                    .find_map(|p| p.trim().strip_prefix("chunk-signature="))
                    .unwrap_or("")
                    .to_string();
                (size.trim(), sig)
            }
            None => (line.trim(), String::new()),
        };
        let size = usize::from_str_radix(size_hex, 16).map_err(|e| e.to_string())?;
        if size == 0 {
            if !sig.is_empty() {
                chunk_signatures.push((Vec::new(), sig));
            }
            break;
        }
        if i + size > body.len() {
            return Err("truncated chunk".into());
        }
        let chunk = body[i..i + size].to_vec();
        i += size;
        if body.get(i..i + 2) != Some(b"\r\n") {
            return Err("missing chunk crlf".into());
        }
        i += 2;
        if !sig.is_empty() {
            chunk_signatures.push((chunk.clone(), sig));
        }
        data.extend_from_slice(&chunk);
    }
    Ok(DecodedChunks {
        data,
        chunk_signatures,
    })
}

pub fn encode_aws_chunk(data: &[u8], signature: Option<&str>) -> Vec<u8> {
    let mut out = Vec::new();
    if let Some(sig) = signature {
        out.extend_from_slice(format!("{:x};chunk-signature={sig}\r\n", data.len()).as_bytes());
    } else {
        out.extend_from_slice(format!("{:x}\r\n", data.len()).as_bytes());
    }
    out.extend_from_slice(data);
    out.extend_from_slice(b"\r\n");
    out
}

pub fn encode_aws_chunked_end(signature: Option<&str>, trailer: Option<&str>) -> Vec<u8> {
    let mut out = Vec::new();
    if let Some(sig) = signature {
        out.extend_from_slice(format!("0;chunk-signature={sig}\r\n\r\n").as_bytes());
    } else {
        out.extend_from_slice(b"0\r\n");
        if let Some(t) = trailer {
            out.extend_from_slice(t.as_bytes());
            if !t.ends_with("\r\n") {
                out.extend_from_slice(b"\r\n");
            }
        }
        out.extend_from_slice(b"\r\n");
    }
    out
}

/// Fixed pump window. Object bytes larger than this are stored as chunks, not one `Vec`.
pub const CHUNK_WINDOW: usize = 64 * 1024;

/// Hidden key for one staged chunk. `staging` is per request so concurrent PUTs do not mix.
pub fn chunk_object_key(staging: &str, index: u32) -> String {
    format!("\0s3c/{staging}/{index:08}")
}

/// Manifest body stored under the user key. Logical length is `total`, not this blob's length.
pub fn encode_manifest(staging: &str, total: u64, window: usize, chunks: u32) -> Vec<u8> {
    format!("S3MF1\n{staging}\n{total}\n{window}\n{chunks}\n").into_bytes()
}

pub fn parse_manifest(body: &[u8]) -> Option<(String, u64, usize, u32)> {
    let text = std::str::from_utf8(body).ok()?;
    let mut lines = text.lines();
    if lines.next()? != "S3MF1" {
        return None;
    }
    let staging = lines.next()?.to_string();
    if staging.is_empty() || staging.contains('/') || staging.contains('\0') {
        return None;
    }
    let total = lines.next()?.parse().ok()?;
    let window = lines.next()?.parse().ok()?;
    let chunks = lines.next()?.parse().ok()?;
    if window == 0 {
        return None;
    }
    Some((staging, total, window, chunks))
}

/// Read up to `limit` bytes from async body; callers stream to storage in chunks.
pub async fn read_body_limited<R: AsyncRead + Unpin>(
    reader: &mut R,
    limit: usize,
) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; CHUNK_WINDOW];
    while buf.len() < limit {
        let n = reader.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        let take = n.min(limit - buf.len());
        buf.extend_from_slice(&tmp[..take]);
        if take < n {
            break;
        }
    }
    Ok(buf)
}

#[cfg(test)]
mod manifest_tests {
    use super::*;

    #[test]
    fn manifest_roundtrip_is_independent_of_object_size() {
        let body = encode_manifest("abc", 10_000_000, CHUNK_WINDOW, 153);
        assert!(body.len() < 128);
        assert_eq!(
            parse_manifest(&body),
            Some(("abc".into(), 10_000_000, CHUNK_WINDOW, 153))
        );
    }
}

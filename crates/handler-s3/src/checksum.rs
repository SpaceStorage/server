//! Content-MD5 and x-amz-checksum-* validation (019 US4).

use sha2::{Digest, Sha256};

use crate::dispatch::S3Reply;

pub fn md5_hex(data: &[u8]) -> String {
    use md5::{Digest as _, Md5};
    format!("{:x}", Md5::digest(data))
}

pub fn md5_raw(data: &[u8]) -> [u8; 16] {
    use md5::{Digest as _, Md5};
    Md5::digest(data).into()
}

pub fn crc32_hex(data: &[u8]) -> String {
    format!("{:08x}", crc32_ieee(data))
}

pub fn verify_content_md5_hex(body: &[u8], expected_hex: &str) -> bool {
    md5_hex(body).eq_ignore_ascii_case(expected_hex.trim())
}

pub fn verify_crc32_hex(body: &[u8], expected: &str) -> bool {
    crc32_hex(body).eq_ignore_ascii_case(expected.trim())
}

pub fn content_md5_base64(data: &[u8]) -> String {
    b64_encode(&md5_raw(data))
}

pub fn checksum_sha256_base64(data: &[u8]) -> String {
    b64_encode(&Sha256::digest(data))
}

pub fn checksum_crc32_base64(data: &[u8]) -> String {
    b64_encode(&crc32_ieee(data).to_be_bytes())
}

pub fn checksum_crc32c_base64(data: &[u8]) -> String {
    b64_encode(&crc32c(data).to_be_bytes())
}

/// Validate checksum headers when the client sent them. Absent headers are ignored.
pub fn check_headers(headers: &[(String, String)], body: &[u8]) -> Result<(), S3Reply> {
    if let Some(v) = header(headers, "content-md5") {
        let got = content_md5_base64(body);
        if !b64_eq(&got, v) {
            return Err(bad_digest("Content-MD5 mismatch"));
        }
    }
    if let Some(v) = header(headers, "x-amz-checksum-sha256") {
        if !b64_eq(&checksum_sha256_base64(body), v) {
            return Err(bad_digest("x-amz-checksum-sha256 mismatch"));
        }
    }
    if let Some(v) = header(headers, "x-amz-checksum-crc32") {
        if !b64_eq(&checksum_crc32_base64(body), v) {
            return Err(bad_digest("x-amz-checksum-crc32 mismatch"));
        }
    }
    if let Some(v) = header(headers, "x-amz-checksum-crc32c") {
        if !b64_eq(&checksum_crc32c_base64(body), v) {
            return Err(bad_digest("x-amz-checksum-crc32c mismatch"));
        }
    }
    Ok(())
}

fn bad_digest(message: &str) -> S3Reply {
    S3Reply::Error {
        code: "BadDigest".into(),
        message: message.into(),
    }
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn b64_eq(got: &str, expected: &str) -> bool {
    let expected = expected.trim();
    if got.eq_ignore_ascii_case(expected) {
        return true;
    }
    match (b64_decode(got), b64_decode(expected)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

fn crc32_ieee(data: &[u8]) -> u32 {
    let mut h = crc32fast::Hasher::new();
    h.update(data);
    h.finalize()
}

/// Castagnoli CRC-32C (reflected poly 0x82F63B78).
pub struct Crc32c {
    crc: u32,
}

impl Crc32c {
    pub fn new() -> Self {
        Self { crc: 0xFFFF_FFFF }
    }

    pub fn update(&mut self, data: &[u8]) {
        for &b in data {
            self.crc ^= u32::from(b);
            for _ in 0..8 {
                let mask = (self.crc & 1).wrapping_neg();
                self.crc = (self.crc >> 1) ^ (0x82F6_3B78 & mask);
            }
        }
    }

    pub fn finish(self) -> u32 {
        !self.crc
    }
}

impl Default for Crc32c {
    fn default() -> Self {
        Self::new()
    }
}

pub fn crc32c(data: &[u8]) -> u32 {
    let mut crc = Crc32c::new();
    crc.update(data);
    crc.finish()
}

/// Checksums accumulated while the body was pumped in windows (no second full-object buffer).
pub fn check_stream_digests(
    headers: &[(String, String)],
    md5_raw: &[u8],
    sha256: &[u8],
    crc32: u32,
    crc32c_sum: u32,
) -> Result<(), S3Reply> {
    if let Some(v) = header(headers, "content-md5") {
        if !b64_eq(&b64_encode(md5_raw), v) {
            return Err(bad_digest("Content-MD5 mismatch"));
        }
    }
    if let Some(v) = header(headers, "x-amz-checksum-sha256") {
        if !b64_eq(&b64_encode(sha256), v) {
            return Err(bad_digest("x-amz-checksum-sha256 mismatch"));
        }
    }
    if let Some(v) = header(headers, "x-amz-checksum-crc32") {
        if !b64_eq(&b64_encode(&crc32.to_be_bytes()), v) {
            return Err(bad_digest("x-amz-checksum-crc32 mismatch"));
        }
    }
    if let Some(v) = header(headers, "x-amz-checksum-crc32c") {
        if !b64_eq(&b64_encode(&crc32c_sum.to_be_bytes()), v) {
            return Err(bad_digest("x-amz-checksum-crc32c mismatch"));
        }
    }
    Ok(())
}

fn b64_encode(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        let n = (u32::from(data[i]) << 16) | (u32::from(data[i + 1]) << 8) | u32::from(data[i + 2]);
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(T[((n >> 6) & 63) as usize] as char);
        out.push(T[(n & 63) as usize] as char);
        i += 3;
    }
    if i < data.len() {
        let b0 = data[i];
        let b1 = if i + 1 < data.len() { data[i + 1] } else { 0 };
        let n = (u32::from(b0) << 16) | (u32::from(b1) << 8);
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        if i + 1 < data.len() {
            out.push(T[((n >> 6) & 63) as usize] as char);
            out.push('=');
        } else {
            out.push('=');
            out.push('=');
        }
    }
    out
}

fn b64_decode(text: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes = text.as_bytes();
    if bytes.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let (c0, c1, c2, c3) = (bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]);
        let b0 = val(c0)?;
        let b1 = val(c1)?;
        out.push((b0 << 2) | (b1 >> 4));
        if c2 != b'=' {
            let b2 = val(c2)?;
            out.push((b1 << 4) | (b2 >> 2));
            if c3 != b'=' {
                let b3 = val(c3)?;
                out.push((b2 << 6) | b3);
            }
        }
        i += 4;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32c_check_value() {
        assert_eq!(crc32c(b"123456789"), 0xE306_9283);
    }

    #[test]
    fn content_md5_matches_and_rejects() {
        let headers = vec![("Content-MD5".into(), content_md5_base64(b"hello"))];
        assert!(check_headers(&headers, b"hello").is_ok());
        let bad = vec![("Content-MD5".into(), "AAAAAAAAAAAAAAAAAAAAAA==".into())];
        assert!(matches!(
            check_headers(&bad, b"hello"),
            Err(S3Reply::Error { code, .. }) if code == "BadDigest"
        ));
    }
}

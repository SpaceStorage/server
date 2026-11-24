//! ETag rules for single-part and multipart composite (019 US4).

use crate::checksum::md5_hex;

pub fn single_part_etag(body: &[u8]) -> String {
    format!("\"{}\"", md5_hex(body))
}

/// AWS composite: MD5 of the concatenated raw part MD5s, quoted, plus `-{count}`.
pub fn multipart_etag(part_md5s: &[String]) -> String {
    let mut raw = Vec::with_capacity(part_md5s.len() * 16);
    for p in part_md5s {
        let hexpart = p.trim_matches('"');
        if let Ok(bytes) = hex::decode(hexpart) {
            raw.extend_from_slice(&bytes);
        }
    }
    let composite = md5_hex(&raw);
    format!("\"{composite}-{}\"", part_md5s.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composite_is_md5_of_raw_part_md5s() {
        // Independent of the hex-string concatenation bug: two one-byte parts.
        let etag = multipart_etag(&[md5_hex(b"a"), md5_hex(b"b")]);
        assert_eq!(etag, "\"96e024ba2074fe77e8e965ba43a704be-2\"");
        assert_ne!(etag, single_part_etag(b"ab"));
    }
}

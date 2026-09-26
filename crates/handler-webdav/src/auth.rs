//! Basic and Digest (RFC 2617 MD5) authentication for WebDAV smoke/demo.

use md5::{Digest, Md5};
use std::collections::HashMap;

pub const REALM: &str = "protocols.webdav.realm";
pub const DEMO_USER: &str = "demo";
pub const DEMO_PASS: &str = "demo";

pub fn md5_hex(data: &str) -> String {
    let digest = Md5::digest(data.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Expected `response` digest for `qop=auth` (RFC 2617).
pub fn digest_response(
    username: &str,
    password: &str,
    realm: &str,
    method: &str,
    uri: &str,
    nonce: &str,
    nc: &str,
    cnonce: &str,
    qop: &str,
) -> String {
    let ha1 = md5_hex(&format!("{username}:{realm}:{password}"));
    let ha2 = md5_hex(&format!("{method}:{uri}"));
    if qop == "auth" || qop == "auth-int" {
        md5_hex(&format!("{ha1}:{nonce}:{nc}:{cnonce}:{qop}:{ha2}"))
    } else {
        md5_hex(&format!("{ha1}:{nonce}:{ha2}"))
    }
}

pub fn basic_challenge() -> String {
    format!(r#"Basic realm="{REALM}""#)
}

pub fn digest_challenge(nonce: &str) -> String {
    format!(
        r#"Digest realm="{REALM}", nonce="{nonce}", algorithm=MD5, qop="auth""#
    )
}

pub fn unauthorized_response_body() -> &'static str {
    "authentication required"
}

pub fn unauthorized_http(nonce: &str) -> String {
    let body = unauthorized_response_body();
    format!(
        "HTTP/1.1 401 Unauthorized\r\n\
         WWW-Authenticate: {}\r\n\
         WWW-Authenticate: {}\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        basic_challenge(),
        digest_challenge(nonce),
        body.len()
    )
}

pub fn verify_basic(header_value: &str) -> bool {
    let token = header_value.trim();
    let Some(b64) = token.strip_prefix("Basic ") else {
        return false;
    };
    let Some(decoded) = base64_decode(b64.trim()) else {
        return false;
    };
    let Ok(s) = String::from_utf8(decoded) else {
        return false;
    };
    match s.split_once(':') {
        Some((u, p)) => u == DEMO_USER && p == DEMO_PASS,
        None => false,
    }
}

pub fn verify_digest(header_value: &str, method: &str, uri: &str, expected_nonce: &str) -> bool {
    let token = header_value.trim();
    let Some(rest) = token.strip_prefix("Digest ") else {
        return false;
    };
    let params = parse_auth_params(rest);
    let Some(username) = params.get("username") else {
        return false;
    };
    if username != DEMO_USER {
        return false;
    }
    if params.get("realm").map(String::as_str) != Some(REALM) {
        return false;
    }
    let Some(nonce) = params.get("nonce") else {
        return false;
    };
    if nonce != expected_nonce {
        return false;
    }
    if let Some(alg) = params.get("algorithm") {
        if !alg.eq_ignore_ascii_case("MD5") {
            return false;
        }
    }
    let Some(response) = params.get("response") else {
        return false;
    };
    let digest_uri = params.get("uri").map(String::as_str).unwrap_or(uri);
    let qop = params.get("qop").map(String::as_str).unwrap_or("");
    let nc = params.get("nc").map(String::as_str).unwrap_or("");
    let cnonce = params.get("cnonce").map(String::as_str).unwrap_or("");

    let expected = if qop == "auth" {
        if nc.is_empty() || cnonce.is_empty() {
            return false;
        }
        digest_response(
            DEMO_USER,
            DEMO_PASS,
            REALM,
            method,
            digest_uri,
            nonce,
            nc,
            cnonce,
            qop,
        )
    } else if qop.is_empty() {
        digest_response(
            DEMO_USER,
            DEMO_PASS,
            REALM,
            method,
            digest_uri,
            nonce,
            "",
            "",
            "",
        )
    } else {
        return false;
    };

    constant_time_eq(response.as_bytes(), expected.as_bytes())
}

pub fn authorize(
    authorization: Option<&str>,
    method: &str,
    uri: &str,
    digest_nonce: &str,
) -> bool {
    let Some(header) = authorization else {
        return false;
    };
    let h = header.trim();
    if h.starts_with("Basic ") {
        verify_basic(h)
    } else if h.starts_with("Digest ") {
        verify_digest(h, method, uri, digest_nonce)
    } else {
        false
    }
}

pub fn new_nonce() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    md5_hex(&format!("nonce:{t}"))
}

fn parse_auth_params(s: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut i = 0;
    let bytes = s.as_bytes();
    while i < bytes.len() {
        while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b',') {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let start = i;
        while i < bytes.len() && bytes[i] != b'=' {
            i += 1;
        }
        let key = s[start..i].trim().to_ascii_lowercase();
        if i >= bytes.len() || bytes[i] != b'=' {
            break;
        }
        i += 1;
        let val = if i < bytes.len() && bytes[i] == b'"' {
            i += 1;
            let vstart = i;
            while i < bytes.len() && bytes[i] != b'"' {
                i += 1;
            }
            let v = s[vstart..i].to_string();
            if i < bytes.len() {
                i += 1;
            }
            v
        } else {
            let vstart = i;
            while i < bytes.len() && bytes[i] != b',' {
                i += 1;
            }
            s[vstart..i].trim().to_string()
        };
        out.insert(key, val);
    }
    out
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn base64_decode(input: &str) -> Option<Vec<u8>> {
    fn val(b: u8) -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::new();
    let mut buf = 0u32;
    let mut bits = 0u32;
    for &b in input.as_bytes() {
        if b == b'=' {
            break;
        }
        let Some(v) = val(b) else {
            continue;
        };
        buf = (buf << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_response_qop_auth_matches_rfc_example_shape() {
        let r = digest_response(
            "demo",
            "demo",
            REALM,
            "GET",
            "/x",
            "abc",
            "00000001",
            "xyz",
            "auth",
        );
        assert_eq!(r.len(), 32);
        assert!(r.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn basic_demo_credentials() {
        assert!(verify_basic("Basic ZGVtbzpkZW1v"));
        assert!(!verify_basic("Basic ZGVtbzpw"));
    }

    #[test]
    fn digest_roundtrip() {
        let nonce = "test-nonce";
        let uri = "/path";
        let nc = "00000001";
        let cnonce = "client1";
        let resp = digest_response(
            DEMO_USER,
            DEMO_PASS,
            REALM,
            "GET",
            uri,
            nonce,
            nc,
            cnonce,
            "auth",
        );
        let header = format!(
            r#"Digest username="{DEMO_USER}", realm="{REALM}", nonce="{nonce}", uri="{uri}", response="{resp}", qop=auth, nc={nc}, cnonce="{cnonce}""#
        );
        assert!(verify_digest(&header, "GET", uri, nonce));
    }

    #[test]
    fn unauthorized_offers_both_schemes() {
        let msg = unauthorized_http("n1");
        assert!(msg.contains("WWW-Authenticate: Basic"));
        assert!(msg.contains("WWW-Authenticate: Digest"));
        assert!(msg.contains("401 Unauthorized"));
    }
}

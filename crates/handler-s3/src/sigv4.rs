//! AWS Signature Version 4 verification (demo credentials `demo` / `demo`).

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

pub const DEMO_ACCESS_KEY: &str = "demo";
pub const DEMO_SECRET_KEY: &str = "demo";
const ALGORITHM: &str = "AWS4-HMAC-SHA256";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedAuthorization {
    pub access_key: String,
    pub date: String,
    pub region: String,
    pub service: String,
    pub signed_headers: Vec<String>,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum SigV4Error {
    MissingAuthorization,
    BadAuthorization,
    MissingDate,
    MissingPayloadHash,
    BadSignature,
    UnknownAccessKey,
}

/// Parsed HTTP fields needed to verify SigV4.
#[derive(Debug, Clone)]
pub struct SigV4Request<'a> {
    pub method: &'a str,
    pub uri_path: &'a str,
    pub query: Option<&'a str>,
    pub headers: &'a [(&'a str, &'a str)],
    pub payload_hash: &'a str,
}

pub fn parse_authorization(value: &str) -> Result<ParsedAuthorization, SigV4Error> {
    if !value.starts_with("AWS4-HMAC-SHA256 ") {
        return Err(SigV4Error::BadAuthorization);
    }
    let rest = &value["AWS4-HMAC-SHA256 ".len()..];
    let mut access_key = None;
    let mut credential_scope = None;
    let mut signed_headers = None;
    let mut signature = None;

    for part in rest.split(',') {
        let part = part.trim();
        if let Some(v) = part.strip_prefix("Credential=") {
            let mut segs = v.split('/');
            access_key = segs.next().map(str::to_string);
            let date = segs.next().ok_or(SigV4Error::BadAuthorization)?;
            let region = segs.next().ok_or(SigV4Error::BadAuthorization)?;
            let service = segs.next().ok_or(SigV4Error::BadAuthorization)?;
            let term = segs.next().ok_or(SigV4Error::BadAuthorization)?;
            if term != "aws4_request" || segs.next().is_some() {
                return Err(SigV4Error::BadAuthorization);
            }
            credential_scope = Some((date.to_string(), region.to_string(), service.to_string()));
        } else if let Some(v) = part.strip_prefix("SignedHeaders=") {
            signed_headers = Some(
                v.split(';')
                    .map(|s| s.trim().to_ascii_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect(),
            );
        } else if let Some(v) = part.strip_prefix("Signature=") {
            signature = Some(v.trim().to_string());
        }
    }

    let access_key = access_key.ok_or(SigV4Error::BadAuthorization)?;
    let (date, region, service) = credential_scope.ok_or(SigV4Error::BadAuthorization)?;
    let signed_headers = signed_headers.ok_or(SigV4Error::BadAuthorization)?;
    let signature = signature.ok_or(SigV4Error::BadAuthorization)?;

    Ok(ParsedAuthorization {
        access_key,
        date,
        region,
        service,
        signed_headers,
        signature,
    })
}

pub fn payload_hash_from_headers(headers: &[(&str, &str)]) -> Result<String, SigV4Error> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("x-amz-content-sha256"))
        .map(|(_, v)| v.to_string())
        .ok_or(SigV4Error::MissingPayloadHash)
}

fn uri_encode(segment: &str, encode_slash: bool) -> String {
    let mut out = String::new();
    for b in segment.bytes() {
        let encode = match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => false,
            b'/' if !encode_slash => false,
            _ => true,
        };
        if encode {
            out.push_str(&format!("%{b:02X}"));
        } else {
            out.push(b as char);
        }
    }
    out
}

fn canonical_uri(path: &str) -> String {
    if path.is_empty() {
        return "/".into();
    }
    let path = path.strip_prefix('/').unwrap_or(path);
    if path.is_empty() {
        return "/".into();
    }
    path.split('/')
        .map(|seg| uri_encode(seg, true))
        .collect::<Vec<_>>()
        .join("/")
        .prepend_if_not_empty()
}

trait PrependSlash {
    fn prepend_if_not_empty(self) -> String;
}

impl PrependSlash for String {
    fn prepend_if_not_empty(self) -> String {
        if self.is_empty() {
            "/".into()
        } else {
            format!("/{self}")
        }
    }
}

fn canonical_query(query: Option<&str>) -> String {
    let Some(q) = query else {
        return String::new();
    };
    if q.is_empty() {
        return String::new();
    }
    let mut pairs: Vec<(String, String)> = q
        .split('&')
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            Some((uri_encode(k, true), uri_encode(v, true)))
        })
        .collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    pairs
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

fn canonical_headers(headers: &[(&str, &str)], signed: &[String]) -> (String, String) {
    let mut lines: Vec<(String, String)> = Vec::new();
    for name in signed {
        let val = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.trim().split_whitespace().collect::<Vec<_>>().join(" "))
            .unwrap_or_default();
        lines.push((name.clone(), val));
    }
    lines.sort_by(|a, b| a.0.cmp(&b.0));
    let canonical = lines
        .iter()
        .map(|(k, v)| format!("{k}:{v}\n"))
        .collect::<String>();
    let signed_list = lines
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(";");
    (canonical, signed_list)
}

pub fn canonical_request(req: &SigV4Request<'_>, signed_headers: &[String]) -> String {
    let method = req.method.to_ascii_uppercase();
    let uri = canonical_uri(req.uri_path);
    let query = canonical_query(req.query);
    let (hdrs, signed_list) = canonical_headers(req.headers, signed_headers);
    format!(
        "{method}\n{uri}\n{query}\n{hdrs}\n{signed_list}\n{}",
        req.payload_hash
    )
}

fn signing_key(secret: &str, date: &str, region: &str, service: &str) -> [u8; 32] {
    let k_date = hmac_sha256(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    hmac_sha256(&k_service, b"aws4_request")
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut mac =
        HmacSha256::new_from_slice(key).expect("HMAC accepts any key length up to the block size");
    mac.update(data);
    mac.finalize().into_bytes().into()
}

fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

pub fn string_to_sign(
    amz_date: &str,
    credential_date: &str,
    region: &str,
    service: &str,
    canonical_req: &str,
) -> String {
    let scope = format!("{credential_date}/{region}/{service}/aws4_request");
    let hash = sha256_hex(canonical_req.as_bytes());
    format!("{ALGORITHM}\n{amz_date}\n{scope}\n{hash}")
}

pub fn compute_signature(
    secret: &str,
    amz_date: &str,
    auth: &ParsedAuthorization,
    canonical_req: &str,
) -> String {
    let sts = string_to_sign(
        amz_date,
        &auth.date,
        &auth.region,
        &auth.service,
        canonical_req,
    );
    let key = signing_key(secret, &auth.date, &auth.region, &auth.service);
    hex::encode(hmac_sha256(&key, sts.as_bytes()))
}

/// Resolve secret for `access_key` (demo only in this slice).
pub fn secret_for_access_key(access_key: &str) -> Option<&'static str> {
    if access_key == DEMO_ACCESS_KEY {
        Some(DEMO_SECRET_KEY)
    } else {
        None
    }
}

pub fn verify_sigv4(req: &SigV4Request<'_>, authorization: &str, amz_date: &str) -> Result<(), SigV4Error> {
    let auth = parse_authorization(authorization)?;
    let secret = secret_for_access_key(&auth.access_key).ok_or(SigV4Error::UnknownAccessKey)?;
    let canonical = canonical_request(req, &auth.signed_headers);
    let expected = compute_signature(secret, amz_date, &auth, &canonical);
    if expected.as_bytes().ct_eq(auth.signature.as_bytes()).unwrap_u8() != 1 {
        return Err(SigV4Error::BadSignature);
    }
    Ok(())
}

/// Build Authorization header value for tests and clients.
#[allow(dead_code)]
pub fn sign_request(
    secret: &str,
    access_key: &str,
    req: &SigV4Request<'_>,
    amz_date: &str,
    credential_date: &str,
    region: &str,
    service: &str,
    signed_headers: &[&str],
) -> String {
    let signed: Vec<String> = signed_headers
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();
    let canonical = canonical_request(req, &signed);
    let auth = ParsedAuthorization {
        access_key: access_key.into(),
        date: credential_date.into(),
        region: region.into(),
        service: service.into(),
        signed_headers: signed.clone(),
        signature: String::new(),
    };
    let sig = compute_signature(secret, amz_date, &auth, &canonical);
    let sh = signed.join(";");
    format!(
        "AWS4-HMAC-SHA256 Credential={access_key}/{credential_date}/{region}/{service}/aws4_request, SignedHeaders={sh}, Signature={sig}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_empty_payload_unsigned() {
        let amz_date = "20220301T120000Z";
        let credential_date = "20220301";
        let host = "localhost";
        let payload_hash = "UNSIGNED-PAYLOAD";
        let headers = [
            ("host", host),
            ("x-amz-date", amz_date),
            ("x-amz-content-sha256", payload_hash),
        ];
        let req = SigV4Request {
            method: "PUT",
            uri_path: "/mybucket/mykey",
            query: None,
            headers: &headers,
            payload_hash,
        };
        let signed = ["host", "x-amz-date", "x-amz-content-sha256"];
        let auth = sign_request(
            DEMO_SECRET_KEY,
            DEMO_ACCESS_KEY,
            &req,
            amz_date,
            credential_date,
            "us-east-1",
            "s3",
            &signed,
        );
        verify_sigv4(&req, &auth, amz_date).expect("round-trip sig");
    }

    #[test]
    fn verify_get_empty_body_sha256() {
        let amz_date = "20220301T120000Z";
        let credential_date = "20220301";
        let empty_hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let headers = [
            ("host", "127.0.0.1"),
            ("x-amz-date", amz_date),
            ("x-amz-content-sha256", empty_hash),
        ];
        let req = SigV4Request {
            method: "GET",
            uri_path: "/bucket/obj",
            query: None,
            headers: &headers,
            payload_hash: empty_hash,
        };
        let auth = sign_request(
            DEMO_SECRET_KEY,
            DEMO_ACCESS_KEY,
            &req,
            amz_date,
            credential_date,
            "us-east-1",
            "s3",
            &["host", "x-amz-date", "x-amz-content-sha256"],
        );
        verify_sigv4(&req, &auth, amz_date).expect("get verify");
    }

    #[test]
    fn bad_signature_rejected() {
        let amz_date = "20220301T120000Z";
        let headers = [
            ("host", "localhost"),
            ("x-amz-date", amz_date),
            ("x-amz-content-sha256", "UNSIGNED-PAYLOAD"),
        ];
        let req = SigV4Request {
            method: "GET",
            uri_path: "/b/k",
            query: None,
            headers: &headers,
            payload_hash: "UNSIGNED-PAYLOAD",
        };
        let auth = "AWS4-HMAC-SHA256 Credential=demo/20220301/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=deadbeef";
        assert_eq!(verify_sigv4(&req, auth, amz_date), Err(SigV4Error::BadSignature));
    }
}

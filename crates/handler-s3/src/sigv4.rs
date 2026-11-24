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
    Expired,
    Skewed,
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
        .map(|seg| uri_encode(&percent_decode(seg), true))
        .collect::<Vec<_>>()
        .join("/")
        .prepend_if_not_empty()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
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
            Some((
                uri_encode(&percent_decode(k), true),
                uri_encode(&percent_decode(v), true),
            ))
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

pub fn verify_sigv4_with_secret(
    req: &SigV4Request<'_>,
    authorization: &str,
    amz_date: &str,
    secret: &str,
) -> Result<(), SigV4Error> {
    let auth = parse_authorization(authorization)?;
    let canonical = canonical_request(req, &auth.signed_headers);
    let expected = compute_signature(secret, amz_date, &auth, &canonical);
    if expected
        .as_bytes()
        .ct_eq(auth.signature.as_bytes())
        .unwrap_u8()
        != 1
    {
        return Err(SigV4Error::BadSignature);
    }
    Ok(())
}

pub fn verify_sigv4(
    req: &SigV4Request<'_>,
    authorization: &str,
    amz_date: &str,
) -> Result<(), SigV4Error> {
    let auth = parse_authorization(authorization)?;
    let secret = secret_for_access_key(&auth.access_key).ok_or(SigV4Error::UnknownAccessKey)?;
    verify_sigv4_with_secret(req, authorization, amz_date, secret)
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

/// `(amz_datetime, credential_date)` in UTC.
pub fn amz_now() -> (String, String) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    amz_from_unix(secs)
}

pub fn amz_from_unix(secs: u64) -> (String, String) {
    let days = (secs / 86400) as i64;
    let tod = (secs % 86400) as u32;
    let (y, m, d) = civil_from_days(days);
    let hh = tod / 3600;
    let mm = (tod % 3600) / 60;
    let ss = tod % 60;
    let date = format!("{y:04}{m:02}{d:02}");
    let amz = format!("{date}T{hh:02}{mm:02}{ss:02}Z");
    (amz, date)
}

fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

pub fn parse_amz_date(amz: &str) -> Option<u64> {
    let amz = amz.trim();
    if amz.len() < 16 {
        return None;
    }
    let y: i32 = amz.get(0..4)?.parse().ok()?;
    let mo: u32 = amz.get(4..6)?.parse().ok()?;
    let d: u32 = amz.get(6..8)?.parse().ok()?;
    if amz.as_bytes().get(8) != Some(&b'T') {
        return None;
    }
    let hh: u32 = amz.get(9..11)?.parse().ok()?;
    let mm: u32 = amz.get(11..13)?.parse().ok()?;
    let ss: u32 = amz.get(13..15)?.parse().ok()?;
    crate::meta::parse_http_date(&format!(
        "Thu, {d:02} {} {y:04} {hh:02}:{mm:02}:{ss:02} GMT",
        match mo {
            1 => "Jan",
            2 => "Feb",
            3 => "Mar",
            4 => "Apr",
            5 => "May",
            6 => "Jun",
            7 => "Jul",
            8 => "Aug",
            9 => "Sep",
            10 => "Oct",
            11 => "Nov",
            12 => "Dec",
            _ => return None,
        }
    ))
}

/// Header-auth clock skew. Presigned expiry is a separate window.
pub fn request_time_skewed(amz_date: &str, now_unix: u64) -> bool {
    let Some(req) = parse_amz_date(amz_date) else {
        return true;
    };
    now_unix.abs_diff(req) > 15 * 60
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresignOk {
    pub access_key: String,
    pub session_token: Option<String>,
}

fn query_pairs(raw: &str) -> Vec<(String, String)> {
    raw.split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            (percent_decode(k), percent_decode(v))
        })
        .collect()
}

fn query_get<'a>(pairs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// Verify a presigned query-string request. `raw_query` is the request target query.
pub fn verify_presigned(
    method: &str,
    uri_path: &str,
    raw_query: &str,
    headers: &[(&str, &str)],
    secret: &str,
    now_unix: u64,
) -> Result<PresignOk, SigV4Error> {
    let pairs = query_pairs(raw_query);
    let algo = query_get(&pairs, "X-Amz-Algorithm").ok_or(SigV4Error::BadAuthorization)?;
    if algo != "AWS4-HMAC-SHA256" {
        return Err(SigV4Error::BadAuthorization);
    }
    let credential = query_get(&pairs, "X-Amz-Credential").ok_or(SigV4Error::BadAuthorization)?;
    let amz_date = query_get(&pairs, "X-Amz-Date").ok_or(SigV4Error::MissingDate)?;
    let expires: u64 = query_get(&pairs, "X-Amz-Expires")
        .unwrap_or("0")
        .parse()
        .map_err(|_| SigV4Error::BadAuthorization)?;
    let signed_headers = query_get(&pairs, "X-Amz-SignedHeaders")
        .ok_or(SigV4Error::BadAuthorization)?
        .split(';')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    let signature = query_get(&pairs, "X-Amz-Signature")
        .ok_or(SigV4Error::BadAuthorization)?
        .to_string();
    let session_token = query_get(&pairs, "X-Amz-Security-Token").map(str::to_string);

    let mut segs = credential.split('/');
    let access_key = segs.next().ok_or(SigV4Error::BadAuthorization)?.to_string();
    let date = segs.next().ok_or(SigV4Error::BadAuthorization)?.to_string();
    let region = segs.next().ok_or(SigV4Error::BadAuthorization)?.to_string();
    let service = segs.next().ok_or(SigV4Error::BadAuthorization)?.to_string();
    let term = segs.next().ok_or(SigV4Error::BadAuthorization)?;
    if term != "aws4_request" || service != "s3" {
        return Err(SigV4Error::BadAuthorization);
    }

    let req_unix = parse_amz_date(amz_date).ok_or(SigV4Error::MissingDate)?;
    if now_unix > req_unix.saturating_add(expires) {
        return Err(SigV4Error::Expired);
    }

    let canonical_query = {
        let mut kept: Vec<(String, String)> = pairs
            .iter()
            .filter(|(k, _)| !k.eq_ignore_ascii_case("X-Amz-Signature"))
            .map(|(k, v)| (uri_encode(k, true), uri_encode(v, true)))
            .collect();
        kept.sort();
        kept.into_iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&")
    };
    let payload_hash = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("x-amz-content-sha256"))
        .map(|(_, v)| (*v).to_string())
        .unwrap_or_else(|| "UNSIGNED-PAYLOAD".into());
    let auth = ParsedAuthorization {
        access_key: access_key.clone(),
        date,
        region,
        service,
        signed_headers,
        signature: String::new(),
    };
    let (hdrs, signed_list) = canonical_headers(headers, &auth.signed_headers);
    let canonical = format!(
        "{}\n{}\n{canonical_query}\n{hdrs}\n{signed_list}\n{payload_hash}",
        method.to_ascii_uppercase(),
        canonical_uri(uri_path),
    );
    let expected = compute_signature(secret, amz_date, &auth, &canonical);
    if expected.as_bytes().ct_eq(signature.as_bytes()).unwrap_u8() != 1 {
        return Err(SigV4Error::BadSignature);
    }
    Ok(PresignOk {
        access_key,
        session_token,
    })
}

pub fn sign_presigned_query(
    secret: &str,
    access_key: &str,
    method: &str,
    uri_path: &str,
    extra_query: &str,
    host: &str,
    amz_date: &str,
    credential_date: &str,
    region: &str,
    expires_secs: u64,
    session_token: Option<&str>,
) -> String {
    let mut parts = vec![
        format!("X-Amz-Algorithm={}", uri_encode("AWS4-HMAC-SHA256", true)),
        format!(
            "X-Amz-Credential={}",
            uri_encode(
                &format!("{access_key}/{credential_date}/{region}/s3/aws4_request"),
                true
            )
        ),
        format!("X-Amz-Date={}", uri_encode(amz_date, true)),
        format!("X-Amz-Expires={expires_secs}"),
        format!("X-Amz-SignedHeaders={}", uri_encode("host", true)),
    ];
    if let Some(tok) = session_token {
        parts.push(format!("X-Amz-Security-Token={}", uri_encode(tok, true)));
    }
    if !extra_query.is_empty() {
        parts.push(extra_query.to_string());
    }
    let raw = parts.join("&");
    let headers = [("host", host)];
    let payload_hash = "UNSIGNED-PAYLOAD";
    let signed = vec!["host".to_string()];
    let auth = ParsedAuthorization {
        access_key: access_key.into(),
        date: credential_date.into(),
        region: region.into(),
        service: "s3".into(),
        signed_headers: signed,
        signature: String::new(),
    };
    let pairs = query_pairs(&raw);
    let mut kept: Vec<(String, String)> = pairs
        .iter()
        .map(|(k, v)| (uri_encode(k, true), uri_encode(v, true)))
        .collect();
    kept.sort();
    let canonical_query = kept
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");
    let (hdrs, signed_list) = canonical_headers(&headers, &auth.signed_headers);
    let canonical = format!(
        "{}\n{}\n{canonical_query}\n{hdrs}\n{signed_list}\n{payload_hash}",
        method.to_ascii_uppercase(),
        canonical_uri(uri_path),
    );
    let sig = compute_signature(secret, amz_date, &auth, &canonical);
    format!("{raw}&X-Amz-Signature={sig}")
}

pub fn chunk_string_to_sign(
    amz_date: &str,
    scope: &str,
    previous_sig: &str,
    chunk: &[u8],
) -> String {
    let empty = sha256_hex(b"");
    let data = sha256_hex(chunk);
    format!("AWS4-HMAC-SHA256-PAYLOAD\n{amz_date}\n{scope}\n{previous_sig}\n{empty}\n{data}")
}

pub fn sign_chunk(
    secret: &str,
    credential_date: &str,
    region: &str,
    service: &str,
    string_to_sign: &str,
) -> String {
    let key = signing_key(secret, credential_date, region, service);
    hex::encode(hmac_sha256(&key, string_to_sign.as_bytes()))
}

pub fn sha256_hex_bytes(data: &[u8]) -> String {
    sha256_hex(data)
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
        assert_eq!(
            verify_sigv4(&req, auth, amz_date),
            Err(SigV4Error::BadSignature)
        );
    }

    #[test]
    fn presign_roundtrip_and_expiry() {
        let (amz, date) = amz_from_unix(1_700_000_000);
        let q = sign_presigned_query(
            DEMO_SECRET_KEY,
            DEMO_ACCESS_KEY,
            "GET",
            "/b/k",
            "",
            "localhost",
            &amz,
            &date,
            "us-east-1",
            60,
            None,
        );
        let headers = [("host", "localhost")];
        verify_presigned("GET", "/b/k", &q, &headers, DEMO_SECRET_KEY, 1_700_000_000).unwrap();
        assert_eq!(
            verify_presigned(
                "GET",
                "/b/k",
                &q,
                &headers,
                DEMO_SECRET_KEY,
                1_700_000_000 + 120
            )
            .unwrap_err(),
            SigV4Error::Expired
        );
    }

    #[test]
    fn skew_window() {
        let (amz, _) = amz_from_unix(1_700_000_000);
        assert!(!request_time_skewed(&amz, 1_700_000_000 + 60));
        assert!(request_time_skewed(&amz, 1_700_000_000 + 16 * 60));
    }

    #[test]
    fn unicode_key_canonical_uri() {
        let uri = canonical_uri("/bucket/a b");
        assert_eq!(uri, "/bucket/a%20b");
        assert_eq!(canonical_uri("/bucket/a%20b"), "/bucket/a%20b");
        let q = canonical_query(Some("prefix=a%20b&marker=x"));
        assert!(q.contains("prefix=a%20b"), "{q}");
    }
}

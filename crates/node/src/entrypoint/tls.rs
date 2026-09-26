//! rustls acceptor from file-referenced PEM cert/key (FR-027 / FR-028).

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio_rustls::TlsAcceptor;
use tracing::warn;

/// Load a `TlsAcceptor` from PEM certificate and key paths.
/// Fails if files are unreadable, PEM is invalid, or the leaf is not currently valid.
pub async fn load_acceptor(
    certificate: &str,
    key: &str,
) -> Result<TlsAcceptor, String> {
    let cert_path = certificate.to_string();
    let key_path = key.to_string();
    tokio::task::spawn_blocking(move || load_acceptor_blocking(&cert_path, &key_path))
        .await
        .map_err(|e| format!("tls load join error: {e}"))?
}

fn load_acceptor_blocking(certificate: &str, key: &str) -> Result<TlsAcceptor, String> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let certs = load_certs(Path::new(certificate))?;
    let key = load_key(Path::new(key))?;
    if certs.is_empty() {
        return Err(format!("certificate '{certificate}': no certificates in PEM"));
    }
    check_leaf_validity(certs[0].as_ref(), certificate)?;

    let cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| format!("invalid cert/key pair: {e}"))?;
    Ok(TlsAcceptor::from(Arc::new(cfg)))
}

fn load_certs(path: &Path) -> Result<Vec<CertificateDer<'static>>, String> {
    let f = File::open(path).map_err(|e| format!("cannot read '{}': {e}", path.display()))?;
    let mut reader = BufReader::new(f);
    rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("certificate '{}': invalid PEM: {e}", path.display()))
}

fn load_key(path: &Path) -> Result<PrivateKeyDer<'static>, String> {
    let f = File::open(path).map_err(|e| format!("cannot read '{}': {e}", path.display()))?;
    let mut reader = BufReader::new(f);
    rustls_pemfile::private_key(&mut reader)
        .map_err(|e| format!("key '{}': invalid PEM: {e}", path.display()))?
        .ok_or_else(|| format!("key '{}': no private key in PEM", path.display()))
}

/// Minimal X.509 Validity extraction (notBefore / notAfter) without pulling `time` 1.88+.
fn check_leaf_validity(der: &[u8], path: &str) -> Result<(), String> {
    let (not_before, not_after) = parse_validity_unix(der)
        .map_err(|e| format!("certificate '{path}': invalid: {e}"))?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    if now < not_before {
        return Err(format!(
            "certificate '{path}' not yet valid (not_before={not_before})"
        ));
    }
    if now > not_after {
        return Err(format!("certificate '{path}' expired at {not_after}"));
    }
    Ok(())
}

fn parse_validity_unix(der: &[u8]) -> Result<(i64, i64), String> {
    // Certificate ::= SEQUENCE { tbsCertificate, ... }
    let (tbs, _) = expect_seq(der)?;
    // TBSCertificate ::= SEQUENCE { [version], serial, sig, issuer, validity, ... }
    let (mut body, _) = expect_seq(tbs)?;
    // optional version [0]
    if body.first() == Some(&0xa0) {
        let (_, rest) = take_tlv(body)?;
        body = rest;
    }
    // serialNumber
    let (_, body) = take_tlv(body)?;
    // signature AlgorithmIdentifier
    let (_, body) = take_tlv(body)?;
    // issuer Name
    let (_, body) = take_tlv(body)?;
    // validity Validity ::= SEQUENCE { notBefore Time, notAfter Time }
    let (validity, _) = expect_seq(body)?;
    let (nb_tlv, rest) = take_tlv(validity)?;
    let (na_tlv, _) = take_tlv(rest)?;
    Ok((parse_time(nb_tlv)?, parse_time(na_tlv)?))
}

fn expect_seq(input: &[u8]) -> Result<(&[u8], &[u8]), String> {
    let (tlv, rest) = take_tlv(input)?;
    if tlv.first() != Some(&0x30) {
        return Err("expected SEQUENCE".into());
    }
    let (hdr, content) = split_tlv(tlv)?;
    let _ = hdr;
    Ok((content, rest))
}

fn take_tlv(input: &[u8]) -> Result<(&[u8], &[u8]), String> {
    if input.len() < 2 {
        return Err("truncated DER".into());
    }
    let (hdr_len, content_len) = der_len(input)?;
    let total = hdr_len + content_len;
    if input.len() < total {
        return Err("truncated DER content".into());
    }
    Ok((&input[..total], &input[total..]))
}

fn split_tlv(tlv: &[u8]) -> Result<(&[u8], &[u8]), String> {
    let (hdr_len, content_len) = der_len(tlv)?;
    Ok((&tlv[..hdr_len], &tlv[hdr_len..hdr_len + content_len]))
}

fn der_len(input: &[u8]) -> Result<(usize, usize), String> {
    if input.is_empty() {
        return Err("empty DER".into());
    }
    let len_byte = *input.get(1).ok_or("truncated DER length")?;
    if len_byte & 0x80 == 0 {
        return Ok((2, len_byte as usize));
    }
    let n = (len_byte & 0x7f) as usize;
    if n == 0 || n > 4 || input.len() < 2 + n {
        return Err("bad DER length".into());
    }
    let mut len = 0usize;
    for b in &input[2..2 + n] {
        len = (len << 8) | *b as usize;
    }
    Ok((2 + n, len))
}

fn parse_time(tlv: &[u8]) -> Result<i64, String> {
    let (tag_and_hdr, content) = split_tlv(tlv)?;
    let tag = tag_and_hdr[0];
    let s = std::str::from_utf8(content).map_err(|_| "time not utf8")?;
    match tag {
        // UTCTime YYMMDDhhmmssZ
        0x17 => parse_utc_time(s),
        // GeneralizedTime YYYYMMDDhhmmssZ
        0x18 => parse_generalized_time(s),
        _ => Err(format!("unknown time tag {tag:#x}")),
    }
}

fn parse_utc_time(s: &str) -> Result<i64, String> {
    let s = s.trim_end_matches('Z');
    if s.len() < 12 {
        return Err("short UTCTime".into());
    }
    let yy: i32 = s[0..2].parse().map_err(|_| "bad UTCTime")?;
    let year = if yy >= 50 { 1900 + yy } else { 2000 + yy };
    let month: u32 = s[2..4].parse().map_err(|_| "bad UTCTime")?;
    let day: u32 = s[4..6].parse().map_err(|_| "bad UTCTime")?;
    let hour: u32 = s[6..8].parse().map_err(|_| "bad UTCTime")?;
    let min: u32 = s[8..10].parse().map_err(|_| "bad UTCTime")?;
    let sec: u32 = s[10..12].parse().map_err(|_| "bad UTCTime")?;
    ymd_hms_unix(year, month, day, hour, min, sec)
}

fn parse_generalized_time(s: &str) -> Result<i64, String> {
    let s = s.trim_end_matches('Z');
    if s.len() < 14 {
        return Err("short GeneralizedTime".into());
    }
    let year: i32 = s[0..4].parse().map_err(|_| "bad GeneralizedTime")?;
    let month: u32 = s[4..6].parse().map_err(|_| "bad GeneralizedTime")?;
    let day: u32 = s[6..8].parse().map_err(|_| "bad GeneralizedTime")?;
    let hour: u32 = s[8..10].parse().map_err(|_| "bad GeneralizedTime")?;
    let min: u32 = s[10..12].parse().map_err(|_| "bad GeneralizedTime")?;
    let sec: u32 = s[12..14].parse().map_err(|_| "bad GeneralizedTime")?;
    ymd_hms_unix(year, month, day, hour, min, sec)
}

fn ymd_hms_unix(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    min: u32,
    sec: u32,
) -> Result<i64, String> {
    if !(1..=12).contains(&month) || day == 0 || day > 31 {
        return Err("bad date".into());
    }
    // Days from civil date (Howard Hinnant) → Unix seconds.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400) as u32;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = (era as i64) * 146097 + doe as i64 - 719468;
    Ok(days * 86400 + hour as i64 * 3600 + min as i64 * 60 + sec as i64)
}

/// Periodic (hourly) leaf expiry check; logs and returns whether expired.
pub fn leaf_expired(certificate: &str) -> bool {
    match load_certs(Path::new(certificate)) {
        Ok(certs) if !certs.is_empty() => match check_leaf_validity(certs[0].as_ref(), certificate)
        {
            Ok(()) => false,
            Err(e) => {
                warn!(error=%e, "entrypoint certificate no longer valid");
                true
            }
        },
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::ymd_hms_unix;

    #[test]
    fn unix_epoch() {
        assert_eq!(ymd_hms_unix(1970, 1, 1, 0, 0, 0).unwrap(), 0);
    }
}

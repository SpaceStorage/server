//! Object metadata envelope stored beside the body bytes (019 T024 / T025).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectMeta {
    pub content_type: String,
    pub cache_control: String,
    pub content_disposition: String,
    pub content_encoding: String,
    pub etag: String,
    pub last_modified: u64,
    pub user_meta: Vec<(String, String)>,
}

impl Default for ObjectMeta {
    fn default() -> Self {
        Self {
            content_type: "application/octet-stream".into(),
            cache_control: String::new(),
            content_disposition: String::new(),
            content_encoding: String::new(),
            etag: String::new(),
            last_modified: 0,
            user_meta: Vec::new(),
        }
    }
}

const MAGIC: &[u8] = b"S3O1";

pub fn encode(meta: &ObjectMeta, body: &[u8]) -> Vec<u8> {
    let mut header = Vec::new();
    push_str(&mut header, &meta.content_type);
    push_str(&mut header, &meta.cache_control);
    push_str(&mut header, &meta.content_disposition);
    push_str(&mut header, &meta.content_encoding);
    push_str(&mut header, &meta.etag);
    header.extend_from_slice(&(meta.user_meta.len() as u16).to_le_bytes());
    for (k, v) in &meta.user_meta {
        push_str(&mut header, k);
        push_str(&mut header, v);
    }
    let mut out = Vec::with_capacity(4 + 8 + 4 + header.len() + body.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&meta.last_modified.to_le_bytes());
    out.extend_from_slice(&(header.len() as u32).to_le_bytes());
    out.extend_from_slice(&header);
    out.extend_from_slice(body);
    out
}

pub fn decode(bytes: &[u8]) -> (ObjectMeta, Vec<u8>) {
    if bytes.len() < 16 || &bytes[..4] != MAGIC {
        return (ObjectMeta::default(), bytes.to_vec());
    }
    let last_modified = u64::from_le_bytes(bytes[4..12].try_into().unwrap_or([0; 8]));
    let header_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap_or([0; 4])) as usize;
    if bytes.len() < 16 + header_len {
        return (ObjectMeta::default(), bytes.to_vec());
    }
    let header = &bytes[16..16 + header_len];
    let body = bytes[16 + header_len..].to_vec();
    let mut meta = ObjectMeta {
        last_modified,
        ..ObjectMeta::default()
    };
    let mut i = 0;
    let Some(ct) = take_str(header, &mut i) else {
        return (ObjectMeta::default(), bytes.to_vec());
    };
    let Some(cc) = take_str(header, &mut i) else {
        return (ObjectMeta::default(), bytes.to_vec());
    };
    let Some(cd) = take_str(header, &mut i) else {
        return (ObjectMeta::default(), bytes.to_vec());
    };
    let Some(ce) = take_str(header, &mut i) else {
        return (ObjectMeta::default(), bytes.to_vec());
    };
    let Some(etag) = take_str(header, &mut i) else {
        return (ObjectMeta::default(), bytes.to_vec());
    };
    meta.content_type = ct;
    meta.cache_control = cc;
    meta.content_disposition = cd;
    meta.content_encoding = ce;
    meta.etag = etag;
    if i + 2 > header.len() {
        return (meta, body);
    }
    let count = u16::from_le_bytes(header[i..i + 2].try_into().unwrap_or([0; 2])) as usize;
    i += 2;
    for _ in 0..count {
        let Some(k) = take_str(header, &mut i) else {
            break;
        };
        let Some(v) = take_str(header, &mut i) else {
            break;
        };
        meta.user_meta.push((k, v));
    }
    (meta, body)
}

fn push_str(out: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    let n = u16::try_from(b.len()).unwrap_or(u16::MAX);
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&b[..n as usize]);
}

fn take_str(buf: &[u8], i: &mut usize) -> Option<String> {
    if *i + 2 > buf.len() {
        return None;
    }
    let n = u16::from_le_bytes(buf[*i..*i + 2].try_into().ok()?) as usize;
    *i += 2;
    if *i + n > buf.len() {
        return None;
    }
    let s = String::from_utf8_lossy(&buf[*i..*i + n]).into_owned();
    *i += n;
    Some(s)
}

pub fn store_value(meta: &ObjectMeta, body: &[u8]) -> Vec<u8> {
    let env = encode(meta, body);
    let mut out = b"S3B64:".to_vec();
    out.extend(b64_encode(&env).into_bytes());
    out
}

pub fn load_value(raw: &[u8]) -> (ObjectMeta, Vec<u8>) {
    if let Ok(text) = std::str::from_utf8(raw) {
        if let Some(rest) = text.strip_prefix("S3B64:") {
            if let Ok(bytes) = b64_decode(rest) {
                return decode(&bytes);
            }
        }
    }
    if raw.starts_with(MAGIC) {
        return decode(raw);
    }
    (ObjectMeta::default(), raw.to_vec())
}

fn b64_encode(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8) | data[i + 2] as u32;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(T[((n >> 6) & 63) as usize] as char);
        out.push(T[(n & 63) as usize] as char);
        i += 3;
    }
    if i < data.len() {
        let rem = data.len() - i;
        let mut n = (data[i] as u32) << 16;
        if rem == 2 {
            n |= (data[i + 1] as u32) << 8;
        }
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        if rem == 2 {
            out.push(T[((n >> 6) & 63) as usize] as char);
            out.push('=');
        } else {
            out.push('=');
            out.push('=');
        }
    }
    out
}

fn b64_decode(text: &str) -> Result<Vec<u8>, ()> {
    fn val(c: u8) -> Result<u8, ()> {
        Ok(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err(()),
        })
    }
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'=' {
            break;
        }
        let b0 = val(*bytes.get(i).ok_or(())?)?;
        let b1 = val(*bytes.get(i + 1).ok_or(())?)?;
        let c2 = *bytes.get(i + 2).ok_or(())?;
        let c3 = *bytes.get(i + 3).ok_or(())?;
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
    Ok(out)
}

pub fn response_headers(meta: &ObjectMeta, body_len: u64) -> Vec<(String, String)> {
    let mut headers = vec![
        ("Content-Type".into(), meta.content_type.clone()),
        ("ETag".into(), meta.etag.clone()),
        ("Last-Modified".into(), http_date(meta.last_modified)),
        ("Content-Length".into(), body_len.to_string()),
    ];
    if !meta.cache_control.is_empty() {
        headers.push(("Cache-Control".into(), meta.cache_control.clone()));
    }
    if !meta.content_disposition.is_empty() {
        headers.push((
            "Content-Disposition".into(),
            meta.content_disposition.clone(),
        ));
    }
    if !meta.content_encoding.is_empty() {
        headers.push(("Content-Encoding".into(), meta.content_encoding.clone()));
    }
    for (k, v) in &meta.user_meta {
        headers.push((format!("x-amz-meta-{k}"), v.clone()));
    }
    headers
}

pub fn iso8601(unix: u64) -> String {
    let (y, m, d, hh, mm, ss, _) = civil_hms(unix);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}.000Z")
}

pub fn http_date(unix: u64) -> String {
    let (y, m, d, hh, mm, ss, wd) = civil_hms(unix);
    const WD: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MON: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
        WD[(wd % 7) as usize],
        d,
        MON[(m.saturating_sub(1) as usize).min(11)],
        y,
        hh,
        mm,
        ss
    )
}

pub fn parse_http_date(s: &str) -> Option<u64> {
    let s = s.trim();
    let rest = s.split_once(',').map(|(_, r)| r.trim()).unwrap_or(s);
    let mut p = rest.split_whitespace();
    let day: u32 = p.next()?.parse().ok()?;
    let mon = month_num(p.next()?)?;
    let year: i32 = p.next()?.parse().ok()?;
    let time = p.next()?;
    let mut t = time.split(':');
    let hh: u32 = t.next()?.parse().ok()?;
    let mm: u32 = t.next()?.parse().ok()?;
    let ss: u32 = t.next()?.trim_end_matches("GMT").parse().ok()?;
    unix_from_civil(year, mon, day, hh, mm, ss)
}

fn month_num(name: &str) -> Option<u32> {
    Some(match name {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    })
}

fn civil_hms(unix: u64) -> (i32, u32, u32, u32, u32, u32, u32) {
    let days = (unix / 86400) as i64;
    let tod = (unix % 86400) as u32;
    let (y, m, d) = civil_from_days(days);
    let hh = tod / 3600;
    let mm = (tod % 3600) / 60;
    let ss = tod % 60;
    let wd = days.rem_euclid(7) as u32;
    (y, m, d, hh, mm, ss, wd)
}

fn unix_from_civil(year: i32, month: u32, day: u32, hh: u32, mm: u32, ss: u32) -> Option<u64> {
    let days = days_from_civil(year, month, day)?;
    if days < 0 {
        return None;
    }
    Some((days as u64) * 86400 + u64::from(hh) * 3600 + u64::from(mm) * 60 + u64::from(ss))
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

fn days_from_civil(year: i32, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || day == 0 || day > 31 {
        return None;
    }
    let y = if month <= 2 { year - 1 } else { year } as i64;
    let m = month as i64;
    let d = day as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy as u64;
    Some(era * 146097 + doe as i64 - 719468)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_roundtrip() {
        let meta = ObjectMeta {
            content_type: "text/plain".into(),
            cache_control: "max-age=9".into(),
            content_disposition: "inline".into(),
            content_encoding: "gzip".into(),
            etag: "\"abc\"".into(),
            last_modified: 1_700_000_000,
            user_meta: vec![("color".into(), "blue".into())],
        };
        let (got, body) = decode(&encode(&meta, b"hello"));
        assert_eq!(body, b"hello");
        assert_eq!(got, meta);
    }

    #[test]
    fn raw_bytes_without_magic_pass_through() {
        let (meta, body) = decode(b"plain");
        assert_eq!(body, b"plain");
        assert!(meta.etag.is_empty());
    }

    #[test]
    fn http_date_epoch() {
        assert_eq!(http_date(0), "Thu, 01 Jan 1970 00:00:00 GMT");
        assert_eq!(parse_http_date("Thu, 01 Jan 1970 00:00:00 GMT"), Some(0));
    }
}

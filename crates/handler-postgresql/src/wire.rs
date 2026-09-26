//! PostgreSQL frontend/backend 3.0 framing (minimal first-binary subset).

use bytes::{BufMut, BytesMut};
use thiserror::Error;

pub const PROTOCOL_VERSION: i32 = 196608; // 3.0
pub const SSL_REQUEST: i32 = 80877103;
pub const CANCEL_REQUEST: i32 = 80877102;
pub const GSSENC_REQUEST: i32 = 80877104;

pub const FEATURE_NOT_SUPPORTED: &str = "0A000";

#[derive(Debug, Error)]
pub enum WireError {
    #[error("incomplete")]
    Incomplete,
    #[error("protocol: {0}")]
    Protocol(String),
}

#[derive(Debug, Clone)]
pub enum FrontendMessage {
    Startup { params: Vec<(String, String)> },
    SslRequest,
    GssRequest,
    CancelRequest { pid: i32, key: i32 },
    PasswordMsg(Vec<u8>),
    Query(String),
    Parse {
        name: String,
        query: String,
        param_types: Vec<i32>,
    },
    Bind {
        portal: String,
        statement: String,
        params: Vec<Option<Vec<u8>>>,
    },
    Describe { typ: u8, name: String },
    Execute { portal: String, max_rows: i32 },
    Sync,
    Close { typ: u8, name: String },
    Flush,
    Terminate,
    Other(u8),
}

pub fn try_read_startup(buf: &mut BytesMut) -> Result<Option<FrontendMessage>, WireError> {
    if buf.len() < 8 {
        return Ok(None);
    }
    let len = i32::from_be_bytes(buf[0..4].try_into().unwrap()) as usize;
    if len < 8 || len > 10_000 {
        return Err(WireError::Protocol(format!("bad startup len {len}")));
    }
    if buf.len() < len {
        return Ok(None);
    }
    let code = i32::from_be_bytes(buf[4..8].try_into().unwrap());
    let body = buf.split_to(len);
    match code {
        SSL_REQUEST => Ok(Some(FrontendMessage::SslRequest)),
        GSSENC_REQUEST => Ok(Some(FrontendMessage::GssRequest)),
        CANCEL_REQUEST => {
            if body.len() < 16 {
                return Err(WireError::Protocol("short cancel".into()));
            }
            let pid = i32::from_be_bytes(body[8..12].try_into().unwrap());
            let key = i32::from_be_bytes(body[12..16].try_into().unwrap());
            Ok(Some(FrontendMessage::CancelRequest { pid, key }))
        }
        PROTOCOL_VERSION => {
            let rest = &body[8..];
            let params = parse_cstr_pairs(rest)?;
            Ok(Some(FrontendMessage::Startup { params }))
        }
        other => Err(WireError::Protocol(format!("unknown startup code {other}"))),
    }
}

pub fn try_read_message(buf: &mut BytesMut) -> Result<Option<FrontendMessage>, WireError> {
    if buf.len() < 5 {
        return Ok(None);
    }
    let tag = buf[0];
    let len = i32::from_be_bytes(buf[1..5].try_into().unwrap()) as usize;
    if len < 4 {
        return Err(WireError::Protocol("bad msg len".into()));
    }
    if buf.len() < 1 + len {
        return Ok(None);
    }
    let _ = buf.split_to(1); // tag
    let body = buf.split_to(len);
    let payload = &body[4..]; // skip length field inside body? Wait - length includes itself
    // body = length(4) + payload(len-4); we already consumed tag; body is full length field + data
    let payload = &body[4..];

    Ok(Some(match tag {
        b'p' => FrontendMessage::PasswordMsg(payload.to_vec()),
        b'Q' => FrontendMessage::Query(cstr(payload)?.to_string()),
        b'P' => parse_parse(payload)?,
        b'B' => parse_bind(payload)?,
        b'D' => {
            if payload.is_empty() {
                return Err(WireError::Protocol("short describe".into()));
            }
            let typ = payload[0];
            let name = cstr(&payload[1..])?.to_string();
            FrontendMessage::Describe { typ, name }
        }
        b'E' => {
            let (portal, n) = read_cstr(payload)?;
            if payload.len() < n + 4 {
                return Err(WireError::Protocol("short execute".into()));
            }
            let max_rows = i32::from_be_bytes(payload[n..n + 4].try_into().unwrap());
            FrontendMessage::Execute {
                portal: portal.to_string(),
                max_rows,
            }
        }
        b'S' => FrontendMessage::Sync,
        b'C' => {
            if payload.is_empty() {
                return Err(WireError::Protocol("short close".into()));
            }
            let typ = payload[0];
            let name = cstr(&payload[1..])?.to_string();
            FrontendMessage::Close { typ, name }
        }
        b'H' => FrontendMessage::Flush,
        b'X' => FrontendMessage::Terminate,
        other => FrontendMessage::Other(other),
    }))
}

fn parse_parse(payload: &[u8]) -> Result<FrontendMessage, WireError> {
    let (name, n1) = read_cstr(payload)?;
    let (query, n2) = read_cstr(&payload[n1..])?;
    let off = n1 + n2;
    if payload.len() < off + 2 {
        return Err(WireError::Protocol("short parse".into()));
    }
    let nparams = u16::from_be_bytes(payload[off..off + 2].try_into().unwrap()) as usize;
    let mut param_types = Vec::with_capacity(nparams);
    let mut o = off + 2;
    for _ in 0..nparams {
        if payload.len() < o + 4 {
            return Err(WireError::Protocol("short parse oids".into()));
        }
        param_types.push(i32::from_be_bytes(payload[o..o + 4].try_into().unwrap()));
        o += 4;
    }
    Ok(FrontendMessage::Parse {
        name: name.to_string(),
        query: query.to_string(),
        param_types,
    })
}

fn parse_bind(payload: &[u8]) -> Result<FrontendMessage, WireError> {
    let (portal, n1) = read_cstr(payload)?;
    let (statement, n2) = read_cstr(&payload[n1..])?;
    let mut o = n1 + n2;
    if payload.len() < o + 2 {
        return Err(WireError::Protocol("short bind".into()));
    }
    let nfmt = u16::from_be_bytes(payload[o..o + 2].try_into().unwrap()) as usize;
    o += 2 + nfmt * 2;
    if payload.len() < o + 2 {
        return Err(WireError::Protocol("short bind params".into()));
    }
    let nparams = u16::from_be_bytes(payload[o..o + 2].try_into().unwrap()) as usize;
    o += 2;
    let mut params = Vec::with_capacity(nparams);
    for _ in 0..nparams {
        if payload.len() < o + 4 {
            return Err(WireError::Protocol("short bind plen".into()));
        }
        let plen = i32::from_be_bytes(payload[o..o + 4].try_into().unwrap());
        o += 4;
        if plen < 0 {
            params.push(None);
        } else {
            let plen = plen as usize;
            if payload.len() < o + plen {
                return Err(WireError::Protocol("short bind pdata".into()));
            }
            params.push(Some(payload[o..o + plen].to_vec()));
            o += plen;
        }
    }
    Ok(FrontendMessage::Bind {
        portal: portal.to_string(),
        statement: statement.to_string(),
        params,
    })
}

fn parse_cstr_pairs(mut rest: &[u8]) -> Result<Vec<(String, String)>, WireError> {
    let mut out = Vec::new();
    while !rest.is_empty() {
        if rest[0] == 0 {
            break;
        }
        let (k, n1) = read_cstr(rest)?;
        let (v, n2) = read_cstr(&rest[n1..])?;
        out.push((k.to_string(), v.to_string()));
        rest = &rest[n1 + n2..];
    }
    Ok(out)
}

fn cstr(buf: &[u8]) -> Result<&str, WireError> {
    Ok(read_cstr(buf)?.0)
}

fn read_cstr(buf: &[u8]) -> Result<(&str, usize), WireError> {
    let end = buf
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| WireError::Protocol("missing cstr nul".into()))?;
    let s = std::str::from_utf8(&buf[..end]).map_err(|_| WireError::Protocol("utf8".into()))?;
    Ok((s, end + 1))
}

pub fn write_auth_ok(out: &mut BytesMut) {
    // AuthenticationOk
    out.put_u8(b'R');
    out.put_i32(8);
    out.put_i32(0);
}

pub fn write_parameter_status(out: &mut BytesMut, key: &str, value: &str) {
    let len = 4 + key.len() + 1 + value.len() + 1;
    out.put_u8(b'S');
    out.put_i32(len as i32);
    out.put_slice(key.as_bytes());
    out.put_u8(0);
    out.put_slice(value.as_bytes());
    out.put_u8(0);
}

pub fn write_backend_key_data(out: &mut BytesMut, pid: i32, key: i32) {
    out.put_u8(b'K');
    out.put_i32(12);
    out.put_i32(pid);
    out.put_i32(key);
}

pub fn write_ready(out: &mut BytesMut, status: u8) {
    out.put_u8(b'Z');
    out.put_i32(5);
    out.put_u8(status); // 'I' idle
}

pub fn write_error(out: &mut BytesMut, sqlstate: &str, message: &str) {
    // Severity, Code, Message, terminator
    let mut fields = BytesMut::new();
    fields.put_u8(b'S');
    fields.put_slice(b"ERROR\0");
    fields.put_u8(b'C');
    fields.put_slice(sqlstate.as_bytes());
    fields.put_u8(0);
    fields.put_u8(b'M');
    fields.put_slice(message.as_bytes());
    fields.put_u8(0);
    fields.put_u8(0);
    out.put_u8(b'E');
    out.put_i32((4 + fields.len()) as i32);
    out.put_slice(&fields);
}

pub fn write_command_complete(out: &mut BytesMut, tag: &str) {
    let len = 4 + tag.len() + 1;
    out.put_u8(b'C');
    out.put_i32(len as i32);
    out.put_slice(tag.as_bytes());
    out.put_u8(0);
}

pub fn write_parse_complete(out: &mut BytesMut) {
    out.put_u8(b'1');
    out.put_i32(4);
}

pub fn write_bind_complete(out: &mut BytesMut) {
    out.put_u8(b'2');
    out.put_i32(4);
}

pub fn write_no_data(out: &mut BytesMut) {
    out.put_u8(b'n');
    out.put_i32(4);
}

pub fn write_row_description(out: &mut BytesMut, cols: &[&str]) {
    let mut body = BytesMut::new();
    body.put_i16(cols.len() as i16);
    for (i, name) in cols.iter().enumerate() {
        body.put_slice(name.as_bytes());
        body.put_u8(0);
        body.put_i32(0); // table oid
        body.put_i16((i + 1) as i16);
        body.put_i32(25); // text oid
        body.put_i16(-1);
        body.put_i32(-1);
        body.put_i16(0); // text format
    }
    out.put_u8(b'T');
    out.put_i32((4 + body.len()) as i32);
    out.put_slice(&body);
}

pub fn write_data_row(out: &mut BytesMut, values: &[Option<&str>]) {
    let mut body = BytesMut::new();
    body.put_i16(values.len() as i16);
    for v in values {
        match v {
            None => body.put_i32(-1),
            Some(s) => {
                body.put_i32(s.len() as i32);
                body.put_slice(s.as_bytes());
            }
        }
    }
    out.put_u8(b'D');
    out.put_i32((4 + body.len()) as i32);
    out.put_slice(&body);
}

pub fn write_ssl_no(out: &mut BytesMut) {
    out.put_u8(b'N');
}

pub fn write_auth_sasl(out: &mut BytesMut) {
    // AuthenticationSASL (10) + mechanisms SCRAM-SHA-256\0\0
    let mech = b"SCRAM-SHA-256\0\0";
    out.put_u8(b'R');
    out.put_i32((4 + 4 + mech.len()) as i32);
    out.put_i32(10);
    out.put_slice(mech);
}

pub fn write_auth_sasl_continue(out: &mut BytesMut, data: &str) {
    out.put_u8(b'R');
    out.put_i32((4 + 4 + data.len()) as i32);
    out.put_i32(11);
    out.put_slice(data.as_bytes());
}

pub fn write_auth_sasl_final(out: &mut BytesMut, data: &str) {
    out.put_u8(b'R');
    out.put_i32((4 + 4 + data.len()) as i32);
    out.put_i32(12);
    out.put_slice(data.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_roundtrip_params() {
        let mut buf = BytesMut::new();
        // length + version + user\0u\0 database\0db\0 \0
        let mut body = BytesMut::new();
        body.put_i32(PROTOCOL_VERSION);
        body.put_slice(b"user\0u\0database\0db\0\0");
        buf.put_i32((4 + body.len()) as i32);
        buf.put_slice(&body);
        let msg = try_read_startup(&mut buf).unwrap().unwrap();
        match msg {
            FrontendMessage::Startup { params } => {
                assert!(params.iter().any(|(k, v)| k == "database" && v == "db"));
            }
            _ => panic!(),
        }
    }
}

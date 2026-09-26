//! CQL native protocol v4/v5 frame codec (minimal HandlersComplete subset).

use bytes::BytesMut;
use thiserror::Error;

pub const VERSION_V4: u8 = 0x04;
pub const VERSION_V5: u8 = 0x05;
pub const RESPONSE_BIT: u8 = 0x80;

pub const OP_ERROR: u8 = 0x00;
pub const OP_STARTUP: u8 = 0x01;
pub const OP_READY: u8 = 0x02;
pub const OP_AUTHENTICATE: u8 = 0x03;
pub const OP_OPTIONS: u8 = 0x05;
pub const OP_SUPPORTED: u8 = 0x06;
pub const OP_QUERY: u8 = 0x07;
pub const OP_RESULT: u8 = 0x08;
pub const OP_AUTH_CHALLENGE: u8 = 0x0E;
pub const OP_AUTH_RESPONSE: u8 = 0x0F;
pub const OP_AUTH_SUCCESS: u8 = 0x10;

pub const ERR_PROTOCOL: i32 = 0x000A;

pub const RESULT_VOID: i32 = 0x0001;
pub const RESULT_ROWS: i32 = 0x0002;

pub const TYPE_VARCHAR: i16 = 0x000d;

pub const HEADER_LEN: usize = 9;
pub const MAX_BODY_LEN: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub version: u8,
    pub flags: u8,
    pub stream: i16,
    pub opcode: u8,
    pub body: Vec<u8>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FrameError {
    #[error("incomplete")]
    Incomplete,
    #[error("protocol: {0}")]
    Protocol(String),
}

pub fn response_version(request_version: u8) -> u8 {
    let major = request_version & 0x7f;
    RESPONSE_BIT | major
}

pub fn encode_frame(version: u8, stream: i16, opcode: u8, flags: u8, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + body.len());
    out.push(version);
    out.push(flags);
    out.extend_from_slice(&stream.to_be_bytes());
    out.push(opcode);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(body);
    out
}

pub fn try_decode_frame(buf: &mut BytesMut) -> Result<Option<Frame>, FrameError> {
    if buf.len() < HEADER_LEN {
        return Ok(None);
    }
    let version = buf[0];
    let flags = buf[1];
    let stream = i16::from_be_bytes([buf[2], buf[3]]);
    let opcode = buf[4];
    let body_len = u32::from_be_bytes([buf[5], buf[6], buf[7], buf[8]]) as usize;
    if body_len > MAX_BODY_LEN {
        return Err(FrameError::Protocol("body too large".into()));
    }
    let total = HEADER_LEN + body_len;
    if buf.len() < total {
        return Ok(None);
    }
    let frame_bytes = buf.split_to(total);
    let body = frame_bytes[HEADER_LEN..].to_vec();
    Ok(Some(Frame {
        version,
        flags,
        stream,
        opcode,
        body,
    }))
}

pub fn write_string(out: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    out.extend_from_slice(&(b.len() as u16).to_be_bytes());
    out.extend_from_slice(b);
}

pub fn write_long_string(out: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    out.extend_from_slice(&(b.len() as i32).to_be_bytes());
    out.extend_from_slice(b);
}

pub fn write_bytes(out: &mut Vec<u8>, data: &[u8]) {
    out.extend_from_slice(&(data.len() as i32).to_be_bytes());
    out.extend_from_slice(data);
}

pub fn write_null_bytes(out: &mut Vec<u8>) {
    out.extend_from_slice(&(-1i32).to_be_bytes());
}

pub fn write_string_map(out: &mut Vec<u8>, pairs: &[(&str, &str)]) {
    for (k, v) in pairs {
        write_string(out, k);
        write_string(out, v);
    }
    write_string(out, "");
}

pub fn read_string(body: &[u8], off: &mut usize) -> Result<String, FrameError> {
    if body.len() < *off + 2 {
        return Err(FrameError::Protocol("short string".into()));
    }
    let n = u16::from_be_bytes([body[*off], body[*off + 1]]) as usize;
    *off += 2;
    if body.len() < *off + n {
        return Err(FrameError::Protocol("truncated string".into()));
    }
    let s = std::str::from_utf8(&body[*off..*off + n])
        .map_err(|_| FrameError::Protocol("bad utf8".into()))?
        .to_string();
    *off += n;
    Ok(s)
}

pub fn read_long_string(body: &[u8], off: &mut usize) -> Result<String, FrameError> {
    if body.len() < *off + 4 {
        return Err(FrameError::Protocol("short long string".into()));
    }
    let n = i32::from_be_bytes([
        body[*off],
        body[*off + 1],
        body[*off + 2],
        body[*off + 3],
    ]) as usize;
    *off += 4;
    if body.len() < *off + n {
        return Err(FrameError::Protocol("truncated long string".into()));
    }
    let s = std::str::from_utf8(&body[*off..*off + n])
        .map_err(|_| FrameError::Protocol("bad utf8".into()))?
        .to_string();
    *off += n;
    Ok(s)
}

pub fn read_bytes(body: &[u8], off: &mut usize) -> Result<Option<Vec<u8>>, FrameError> {
    if body.len() < *off + 4 {
        return Err(FrameError::Protocol("short bytes".into()));
    }
    let n = i32::from_be_bytes([
        body[*off],
        body[*off + 1],
        body[*off + 2],
        body[*off + 3],
    ]);
    *off += 4;
    if n < 0 {
        return Ok(None);
    }
    let n = n as usize;
    if body.len() < *off + n {
        return Err(FrameError::Protocol("truncated bytes".into()));
    }
    let b = body[*off..*off + n].to_vec();
    *off += n;
    Ok(Some(b))
}

pub fn parse_options(_body: &[u8]) -> Result<(), FrameError> {
    Ok(())
}

pub fn parse_startup(body: &[u8]) -> Result<Vec<(String, String)>, FrameError> {
    let mut off = 0;
    let mut out = Vec::new();
    loop {
        if off + 2 > body.len() {
            return Err(FrameError::Protocol("short startup map".into()));
        }
        let key_len = u16::from_be_bytes([body[off], body[off + 1]]) as usize;
        off += 2;
        if key_len == 0 {
            break;
        }
        if off + key_len > body.len() {
            return Err(FrameError::Protocol("truncated startup key".into()));
        }
        let key = std::str::from_utf8(&body[off..off + key_len])
            .map_err(|_| FrameError::Protocol("bad utf8".into()))?
            .to_string();
        off += key_len;
        let val = read_string(body, &mut off)?;
        out.push((key, val));
    }
    Ok(out)
}

pub fn parse_query_cql(body: &[u8]) -> Result<String, FrameError> {
    let mut off = 0;
    read_long_string(body, &mut off)
}

pub fn encode_supported(version: u8, stream: i16) -> Vec<u8> {
    let mut body = Vec::new();
    write_string_map(
        &mut body,
        &[
            ("CQL_VERSION", "3.4.6"),
            ("COMPRESSION", ""),
            ("PROTOCOL_VERSION", if (version & 0x7f) == VERSION_V5 as u8 {
                "5"
            } else {
                "4"
            }),
        ],
    );
    encode_frame(response_version(version), stream, OP_SUPPORTED, 0, &body)
}

pub fn encode_authenticate(version: u8, stream: i16) -> Vec<u8> {
    let mut body = Vec::new();
    write_string(
        &mut body,
        "org.apache.cassandra.auth.PasswordAuthenticator",
    );
    encode_frame(response_version(version), stream, OP_AUTHENTICATE, 0, &body)
}

pub fn encode_auth_success(version: u8, stream: i16) -> Vec<u8> {
    let mut body = Vec::new();
    write_null_bytes(&mut body);
    encode_frame(response_version(version), stream, OP_AUTH_SUCCESS, 0, &body)
}

pub fn encode_ready(version: u8, stream: i16) -> Vec<u8> {
    encode_frame(response_version(version), stream, OP_READY, 0, &[])
}

pub fn encode_error(version: u8, stream: i16, code: i32, message: &str) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&code.to_be_bytes());
    write_string(&mut body, message);
    encode_frame(response_version(version), stream, OP_ERROR, 0, &body)
}

pub fn encode_protocol_error(version: u8, stream: i16, message: &str) -> Vec<u8> {
    encode_error(version, stream, ERR_PROTOCOL, message)
}

pub fn encode_void_result(version: u8, stream: i16) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&RESULT_VOID.to_be_bytes());
    encode_frame(response_version(version), stream, OP_RESULT, 0, &body)
}

pub fn encode_rows_result(version: u8, stream: i16, columns: &[(&str, i16)], rows: &[Vec<(&str, &str)>]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&RESULT_ROWS.to_be_bytes());
    // metadata flags: no GLOBAL_TABLES_SPEC
    body.extend_from_slice(&0i32.to_be_bytes());
    body.extend_from_slice(&(columns.len() as i32).to_be_bytes());
    for (idx, (name, ty)) in columns.iter().enumerate() {
        write_string(&mut body, "");
        write_string(&mut body, "");
        write_string(&mut body, name);
        body.extend_from_slice(&(idx as i16).to_be_bytes());
        body.extend_from_slice(&ty.to_be_bytes());
    }
    body.extend_from_slice(&(rows.len() as i32).to_be_bytes());
    for row in rows {
        for (col_name, val) in row {
            let _ = col_name;
            write_bytes(&mut body, val.as_bytes());
        }
    }
    encode_frame(response_version(version), stream, OP_RESULT, 0, &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_header_and_void() {
        let raw = encode_void_result(VERSION_V4, 7);
        assert_eq!(raw[0], 0x84);
        assert_eq!(raw[4], OP_RESULT);
        let mut buf = BytesMut::from(raw.as_slice());
        let f = try_decode_frame(&mut buf).unwrap().unwrap();
        assert_eq!(f.opcode, OP_RESULT);
        assert_eq!(f.stream, 7);
        assert!(buf.is_empty());
    }

    #[test]
    fn encode_error_protocol() {
        let raw = encode_protocol_error(VERSION_V5, 0, "protocol mismatch");
        assert_eq!(raw[0], 0x85);
        assert_eq!(raw[4], OP_ERROR);
        let code = i32::from_be_bytes([raw[9], raw[10], raw[11], raw[12]]);
        assert_eq!(code, ERR_PROTOCOL);
    }

    #[test]
    fn parse_startup_map() {
        let mut body = Vec::new();
        write_string_map(
            &mut body,
            &[("CQL_VERSION", "3.0.0"), ("DRIVER_NAME", "test")],
        );
        let opts = parse_startup(&body).unwrap();
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0].0, "CQL_VERSION");
    }

    #[test]
    fn rows_result_body_kind() {
        let raw = encode_rows_result(
            VERSION_V4,
            1,
            &[("id", TYPE_VARCHAR), ("v", TYPE_VARCHAR)],
            &[vec![("id", "1"), ("v", "a")]],
        );
        let kind = i32::from_be_bytes([raw[9], raw[10], raw[11], raw[12]]);
        assert_eq!(kind, RESULT_ROWS);
    }
}

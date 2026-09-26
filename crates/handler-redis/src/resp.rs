//! Minimal RESP2 codec (avoids redis-protocol resp3 feature coupling).

use bytes::{BufMut, BytesMut};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RespValue {
    SimpleString(String),
    Error(String),
    Integer(i64),
    BulkString(Option<Vec<u8>>),
    Array(Vec<RespValue>),
}

#[derive(Debug, Error)]
pub enum RespError {
    #[error("incomplete")]
    Incomplete,
    #[error("protocol: {0}")]
    Protocol(String),
}

pub fn encode(value: &RespValue, out: &mut BytesMut) {
    match value {
        RespValue::SimpleString(s) => {
            out.put_u8(b'+');
            out.put_slice(s.as_bytes());
            out.put_slice(b"\r\n");
        }
        RespValue::Error(s) => {
            out.put_u8(b'-');
            out.put_slice(s.as_bytes());
            out.put_slice(b"\r\n");
        }
        RespValue::Integer(i) => {
            out.put_u8(b':');
            out.put_slice(i.to_string().as_bytes());
            out.put_slice(b"\r\n");
        }
        RespValue::BulkString(None) => {
            out.put_slice(b"$-1\r\n");
        }
        RespValue::BulkString(Some(b)) => {
            out.put_u8(b'$');
            out.put_slice(b.len().to_string().as_bytes());
            out.put_slice(b"\r\n");
            out.put_slice(b);
            out.put_slice(b"\r\n");
        }
        RespValue::Array(items) => {
            out.put_u8(b'*');
            out.put_slice(items.len().to_string().as_bytes());
            out.put_slice(b"\r\n");
            for it in items {
                encode(it, out);
            }
        }
    }
}

pub fn try_decode(buf: &mut BytesMut) -> Result<Option<RespValue>, RespError> {
    if buf.is_empty() {
        return Ok(None);
    }
    let (val, n) = decode_at(buf, 0)?;
    let _ = buf.split_to(n);
    Ok(Some(val))
}

fn decode_at(buf: &[u8], start: usize) -> Result<(RespValue, usize), RespError> {
    if start >= buf.len() {
        return Err(RespError::Incomplete);
    }
    match buf[start] {
        b'+' => {
            let (line, end) = read_line(buf, start + 1)?;
            Ok((
                RespValue::SimpleString(String::from_utf8_lossy(line).into_owned()),
                end,
            ))
        }
        b'-' => {
            let (line, end) = read_line(buf, start + 1)?;
            Ok((
                RespValue::Error(String::from_utf8_lossy(line).into_owned()),
                end,
            ))
        }
        b':' => {
            let (line, end) = read_line(buf, start + 1)?;
            let n: i64 = std::str::from_utf8(line)
                .map_err(|_| RespError::Protocol("utf8".into()))?
                .parse()
                .map_err(|_| RespError::Protocol("int".into()))?;
            Ok((RespValue::Integer(n), end))
        }
        b'$' => {
            let (line, end) = read_line(buf, start + 1)?;
            let len: i64 = std::str::from_utf8(line)
                .map_err(|_| RespError::Protocol("utf8".into()))?
                .parse()
                .map_err(|_| RespError::Protocol("bulk len".into()))?;
            if len < 0 {
                return Ok((RespValue::BulkString(None), end));
            }
            let len = len as usize;
            if buf.len() < end + len + 2 {
                return Err(RespError::Incomplete);
            }
            let data = buf[end..end + len].to_vec();
            Ok((RespValue::BulkString(Some(data)), end + len + 2))
        }
        b'*' => {
            let (line, end) = read_line(buf, start + 1)?;
            let n: i64 = std::str::from_utf8(line)
                .map_err(|_| RespError::Protocol("utf8".into()))?
                .parse()
                .map_err(|_| RespError::Protocol("array len".into()))?;
            if n < 0 {
                return Ok((RespValue::BulkString(None), end));
            }
            let mut items = Vec::with_capacity(n as usize);
            let mut off = end;
            for _ in 0..n {
                let (v, n2) = decode_at(buf, off)?;
                items.push(v);
                off = n2;
            }
            Ok((RespValue::Array(items), off))
        }
        other => Err(RespError::Protocol(format!("bad type {}", other as char))),
    }
}

fn read_line(buf: &[u8], start: usize) -> Result<(&[u8], usize), RespError> {
    let mut i = start;
    while i + 1 < buf.len() {
        if buf[i] == b'\r' && buf[i + 1] == b'\n' {
            return Ok((&buf[start..i], i + 2));
        }
        i += 1;
    }
    Err(RespError::Incomplete)
}

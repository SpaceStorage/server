use bytes::{Buf, BufMut, Bytes, BytesMut};
use thiserror::Error;

pub const MAX_FRAME: usize = 4 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum FrameError {
    #[error("frame too large")]
    TooLarge,
    #[error("incomplete frame")]
    Incomplete,
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn encode_frame(payload: &[u8]) -> Result<Bytes, FrameError> {
    if payload.len() > MAX_FRAME || payload.is_empty() {
        return Err(FrameError::TooLarge);
    }
    let mut buf = BytesMut::with_capacity(4 + payload.len());
    buf.put_u32(payload.len() as u32);
    buf.extend_from_slice(payload);
    Ok(buf.freeze())
}

pub fn decode_frame(buf: &mut BytesMut) -> Result<Option<Bytes>, FrameError> {
    if buf.len() < 4 {
        return Ok(None);
    }
    let mut peek = &buf[..4];
    let len = peek.get_u32() as usize;
    if len == 0 || len > MAX_FRAME {
        return Err(FrameError::TooLarge);
    }
    if buf.len() < 4 + len {
        return Ok(None);
    }
    buf.advance(4);
    let payload = buf.split_to(len).freeze();
    Ok(Some(payload))
}

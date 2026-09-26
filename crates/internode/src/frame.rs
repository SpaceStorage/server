//! Length-prefixed frame: `u32le length | u8 version | u16le msg_type | payload`.

use thiserror::Error;

pub const CURRENT_VERSION: u8 = 1;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FrameError {
    #[error("version_incompatible")]
    VersionIncompatible,
    #[error("truncated")]
    Truncated,
    #[error("payload too large")]
    TooLarge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub version: u8,
    pub msg_type: u16,
    pub payload: Vec<u8>,
}

pub fn encode_frame(msg_type: u16, payload: &[u8]) -> Vec<u8> {
    encode_frame_version(CURRENT_VERSION, msg_type, payload)
}

pub fn encode_frame_version(version: u8, msg_type: u16, payload: &[u8]) -> Vec<u8> {
    let len = (1 + 2 + payload.len()) as u32;
    let mut out = Vec::with_capacity(4 + len as usize);
    out.extend_from_slice(&len.to_le_bytes());
    out.push(version);
    out.extend_from_slice(&msg_type.to_le_bytes());
    out.extend_from_slice(payload);
    out
}

pub fn decode_frame(buf: &[u8]) -> Result<(Frame, usize), FrameError> {
    if buf.len() < 4 {
        return Err(FrameError::Truncated);
    }
    let len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if len < 3 {
        return Err(FrameError::Truncated);
    }
    if buf.len() < 4 + len {
        return Err(FrameError::Truncated);
    }
    let version = buf[4];
    // Accept current and previous (1 and 0).
    if version != CURRENT_VERSION && version != 0 {
        return Err(FrameError::VersionIncompatible);
    }
    let msg_type = u16::from_le_bytes([buf[5], buf[6]]);
    let payload = buf[7..4 + len].to_vec();
    Ok((
        Frame {
            version,
            msg_type,
            payload,
        },
        4 + len,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_version_window() {
        let enc = encode_frame(7, b"hi");
        let (f, n) = decode_frame(&enc).unwrap();
        assert_eq!(n, enc.len());
        assert_eq!(f.msg_type, 7);
        assert_eq!(f.payload, b"hi");

        let v0 = encode_frame_version(0, 1, b"x");
        assert!(decode_frame(&v0).is_ok());

        let bad = encode_frame_version(9, 1, b"x");
        assert_eq!(
            decode_frame(&bad).unwrap_err(),
            FrameError::VersionIncompatible
        );
    }
}

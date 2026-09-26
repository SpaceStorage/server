//! Message type registry; unknown → Ack unknown_message.

pub const MSG_HEARTBEAT: u16 = 1;
pub const MSG_ACK: u16 = 2;
pub const MSG_FANOUT_WRITE: u16 = 3;
pub const MSG_FANOUT_READ: u16 = 4;
pub const MSG_FORWARD_WRITE: u16 = 5;
pub const MSG_JOIN_REQUEST: u16 = 10;
pub const MSG_JOIN_ACK: u16 = 11;
pub const MSG_PENDING_ANNOUNCE: u16 = 12;
pub const MSG_FENCE_INCARNATION: u16 = 13;
pub const MSG_SECRET_ROTATE: u16 = 14;

pub fn is_known(msg_type: u16) -> bool {
    matches!(
        msg_type,
        MSG_HEARTBEAT
            | MSG_ACK
            | MSG_FANOUT_WRITE
            | MSG_FANOUT_READ
            | MSG_FORWARD_WRITE
            | MSG_JOIN_REQUEST
            | MSG_JOIN_ACK
            | MSG_PENDING_ANNOUNCE
            | MSG_FENCE_INCARNATION
            | MSG_SECRET_ROTATE
    )
}

pub fn unknown_ack_payload() -> &'static [u8] {
    br#"{"error":"unknown_message"}"#
}

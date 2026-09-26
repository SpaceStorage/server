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
/// Raft RequestVote (006).
pub const MSG_RAFT_VOTE: u16 = 20;
/// Raft AppendEntries (006).
pub const MSG_RAFT_APPEND: u16 = 21;
/// Raft InstallSnapshot (006).
pub const MSG_RAFT_SNAPSHOT: u16 = 22;
/// Metadata write forward to leader (006).
pub const MSG_RAFT_FORWARD: u16 = 23;
/// Shared-datatype metrics push to namespace primary (006).
pub const MSG_METRICS_PUSH: u16 = 24;
/// Best-effort quota usage delta (007).
pub const MSG_QUOTA_DELTA: u16 = 25;
/// Shuffle offer (005).
pub const MSG_SHUFFLE_OFFER: u16 = 30;
/// Shuffle push (005).
pub const MSG_SHUFFLE_PUSH: u16 = 31;
/// Shuffle pull (005).
pub const MSG_SHUFFLE_PULL: u16 = 32;
/// Stage abort (005).
pub const MSG_STAGE_ABORT: u16 = 33;
/// Job status notify (005).
pub const MSG_JOB_STATUS: u16 = 34;

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
            | MSG_RAFT_VOTE
            | MSG_RAFT_APPEND
            | MSG_RAFT_SNAPSHOT
            | MSG_RAFT_FORWARD
            | MSG_METRICS_PUSH
            | MSG_QUOTA_DELTA
            | MSG_SHUFFLE_OFFER
            | MSG_SHUFFLE_PUSH
            | MSG_SHUFFLE_PULL
            | MSG_STAGE_ABORT
            | MSG_JOB_STATUS
    )
}

pub fn unknown_ack_payload() -> &'static [u8] {
    br#"{"error":"unknown_message"}"#
}

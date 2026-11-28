//! Redis dialect (`002`) — RESP2 first-binary K/V MUST (`016`) and the
//! complete-product capability surface (`017`).

mod block;
mod commands;
mod expiry;
mod handler;
mod lease;
mod lower;
mod pubsub;
mod render;
mod resp;
mod session;
mod stats;

pub use commands::{MUST_COMMANDS, RedisReply, SessionState, dispatch};
pub use commands::txn::{QueuedCommand, TransactionSession, TxnState};
pub use block::{BlockKind, BlockRegistry, BlockRequest, BlockingWait, effective_timeout, parse_block};
pub use expiry::{Deadline, TtlInstant, now_unix_millis, report_ttl};
pub use handler::RedisHandler;
pub use lease::{LeaseError, StreamLeaseAuthority, StreamLeaseError, StreamLeaseProvider};
pub use pubsub::{PubSubBus, PushFrame};
pub use stats::{CmdStat, RedisStats, RedisStatsSnapshot};
// `017` US4: the node-introspection seam is consumed by `crates/node` through
// this crate (mirroring the `StreamLeaseProvider` re-export above), so the
// handler crate itself gains no node/control-plane dependency.
pub use spacestorage_protocol_core::{NodeInfoProvider, NodeInfoSnapshot};
pub use lower::{
    BitmapValue, ConsumerGroup, EntryPayload, HashField, HashValue, HllValue, ListValue,
    PendingEntry, RedisEntry, RedisKeySpace, RedisVisibleType, SetValue, StreamEntry, StreamId,
    StreamOrder, StreamValue, ZSetValue,
};
pub use render::WRONGTYPE;
pub use session::{ConnectionId, PubSubMessage, RedisSession, Subscription};

pub const WIRE_VERSION: &str = "7.2.0-spacestorage";

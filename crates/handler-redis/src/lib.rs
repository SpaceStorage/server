//! Redis first-binary dialect (002) — RESP2 K/V MUST on `K/V Store`.

mod commands;
mod handler;
mod resp;

pub use commands::{dispatch, RedisReply, SessionState, MUST_COMMANDS};
pub use handler::RedisHandler;

pub const WIRE_VERSION: &str = "7.2.0-spacestorage";

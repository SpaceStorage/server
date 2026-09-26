//! Redis first-binary dialect (002) — RESP2 K/V MUST on `K/V Store`.

mod commands;
mod handler;
mod resp;

pub use commands::{MUST_COMMANDS, RedisReply, SessionState, dispatch};
pub use handler::RedisHandler;

pub const WIRE_VERSION: &str = "7.2.0-spacestorage";

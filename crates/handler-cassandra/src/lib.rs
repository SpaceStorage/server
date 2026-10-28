//! Cassandra HandlersComplete dialect — CQL DML/DDL over catalog IR (classify-gated).

mod cql;
mod dispatch;
mod frame;
mod handler;

pub use dispatch::{CassandraReply, SessionState, dispatch};
pub use handler::CassandraHandler;

/// CQL native frame codec (integration / wire conformance tests).
#[doc(hidden)]
pub mod wire {
    pub use crate::frame::*;
}

pub const WIRE_VERSION: &str = "v4";
pub const HANDLER_NAME: &str = "cassandra";

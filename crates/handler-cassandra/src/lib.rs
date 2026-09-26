//! Cassandra HandlersComplete dialect — CQL DML/DDL over catalog IR (classify-gated).

mod cql;
mod dispatch;
mod frame;
mod handler;

pub use dispatch::{CassandraReply, SessionState, dispatch};
pub use handler::CassandraHandler;

pub const WIRE_VERSION: &str = "v4";
pub const HANDLER_NAME: &str = "cassandra";

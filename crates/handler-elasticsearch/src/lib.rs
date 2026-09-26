//! Elasticsearch HandlersComplete — document CRUD + MUST search (aggs deferred to CP).

mod dispatch;
mod handler;

pub use dispatch::{EsReply, SessionState, dispatch};
pub use handler::ElasticsearchHandler;

pub const WIRE_VERSION: &str = "HTTP/1.1";
pub const HANDLER_NAME: &str = "elasticsearch";

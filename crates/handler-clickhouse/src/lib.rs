//! ClickHouse native + HTTP HandlersComplete (CRUD/WHERE; GROUP BY → not-supported until CP).

mod dispatch;
mod http;
mod native;

pub use dispatch::{ClickHouseReply, SessionState, dispatch};
pub use http::ClickHouseHttpHandler;
pub use native::ClickHouseNativeHandler;

pub const WIRE_VERSION_NATIVE: &str = "native";
pub const WIRE_VERSION_HTTP: &str = "HTTP";
pub const HANDLER_NAME_NATIVE: &str = "clickhouse";
pub const HANDLER_NAME_HTTP: &str = "clickhouse-http";

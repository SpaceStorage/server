//! ClickHouse native + HTTP HandlersComplete (CRUD/WHERE; GROUP BY → not-supported until CP).

mod cityhash102;
mod dispatch;
mod http;
mod native;
mod sql;

pub use cityhash102::{city_hash128, clickhouse_block_checksum};
pub use dispatch::{dispatch, ClickHouseReply, SessionState};
pub use http::ClickHouseHttpHandler;
pub use native::ClickHouseNativeHandler;

pub const WIRE_VERSION_NATIVE: &str = "native";
pub const WIRE_VERSION_HTTP: &str = "HTTP";
pub const HANDLER_NAME_NATIVE: &str = "clickhouse";
pub const HANDLER_NAME_HTTP: &str = "clickhouse-http";

//! WebDAV RFC 4918 subset (HandlersComplete).

mod auth;
mod dispatch;
mod handler;

pub use dispatch::{SessionState, WebDavReply, dispatch};
pub use handler::WebDavHandler;

pub const WIRE_VERSION: &str = "RFC4918";
pub const HANDLER_NAME: &str = "webdav";

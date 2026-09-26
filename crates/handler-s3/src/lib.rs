//! S3 SigV4 HandlersComplete — bucket/object MUST verbs (versioning/lock MUST NOT).

mod dispatch;
mod handler;

pub use dispatch::{S3Reply, SessionState, dispatch};
pub use handler::S3Handler;

pub const WIRE_VERSION: &str = "SigV4";
pub const HANDLER_NAME: &str = "s3";

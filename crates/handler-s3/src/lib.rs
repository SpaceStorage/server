//! S3 SigV4 HandlersComplete — bucket/object MUST verbs (versioning/lock MUST NOT).

mod dispatch;
mod handler;
mod sigv4;

pub use dispatch::{S3Reply, SessionState, dispatch};
pub use handler::S3Handler;
pub use sigv4::{sign_request, SigV4Request, DEMO_ACCESS_KEY, DEMO_SECRET_KEY};

pub const WIRE_VERSION: &str = "SigV4";
pub const HANDLER_NAME: &str = "s3";

//! S3 SigV4 HandlersComplete — bucket/object MUST verbs (versioning/lock MUST NOT).

mod auth;
mod checksum;
mod dispatch;
mod etag;
mod handler;
mod meta;
mod multipart;
mod router;
mod sigv4;
mod stream;
mod xml;

pub use auth::{CredentialStore, S3Credential, check_session_token};
pub use checksum::{
    checksum_crc32_base64, checksum_crc32c_base64, checksum_sha256_base64, content_md5_base64,
};
pub use dispatch::{S3Reply, SessionState, apply_query_headers, dispatch, put_object_bytes};
pub use handler::S3Handler;
pub use sigv4::{
    DEMO_ACCESS_KEY, DEMO_SECRET_KEY, SigV4Request, amz_from_unix, amz_now, chunk_string_to_sign,
    sign_chunk, sign_presigned_query, sign_request,
};
pub use stream::CHUNK_WINDOW;

pub const WIRE_VERSION: &str = "SigV4";
pub const HANDLER_NAME: &str = "s3";

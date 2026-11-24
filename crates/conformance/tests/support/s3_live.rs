//! Live in-process S3 TCP harness + SHA256 verify (019 T010, FR-042).

use sha2::{Digest, Sha256};
use spacestorage_handler_s3::{S3Handler, sign_request, SigV4Request, DEMO_ACCESS_KEY, DEMO_SECRET_KEY};
use spacestorage_types::ContainerCatalog;
use std::sync::{Arc, RwLock};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

/// FR-042: call on every successful Suite A read of bytes that were written.
pub fn sha256_record_and_verify(written: &[u8], read_back: &[u8]) {
    let expect = sha256_hex(written);
    let got = sha256_hex(read_back);
    assert_eq!(expect, got, "SHA256 mismatch");
}

/// SHA256 of an identity (non-chunked) HTTP body against the bytes that were written.
pub fn sha256_verify_http_body(written: &[u8], response: &str) {
    let start = response.find("\r\n\r\n").map(|i| i + 4).unwrap_or(0);
    sha256_record_and_verify(written, response[start..].as_bytes());
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

pub async fn spawn_s3_handler(
    handler: S3Handler,
    cancel: CancellationToken,
) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                accept = listener.accept() => {
                    let Ok((sock, _)) = accept else { break };
                    let h = S3Handler {
                        catalog: Arc::clone(&handler.catalog),
                        profile: handler.profile,
                        namespace: handler.namespace.clone(),
                        limit_services: handler.limit_services.clone(),
                        tenancy: handler.tenancy.clone(),
                        region: handler.region.clone(),
                        virtual_host_suffix: handler.virtual_host_suffix.clone(),
                        multipart_idle: handler.multipart_idle,
                        multipart_min_part: handler.multipart_min_part,
                        multipart_max_parts: handler.multipart_max_parts,
                        credentials: Arc::clone(&handler.credentials),
                        multipart_registry: Arc::clone(&handler.multipart_registry),
                    };
                    let c = cancel.child_token();
                    tokio::spawn(async move { h.serve(sock, c).await });
                }
            }
        }
    });
    addr
}

pub fn build_signed_put(path: &str, body: &[u8], host: &str) -> String {
    let (amz_date, credential_date) = spacestorage_handler_s3::amz_now();
    let payload_hash = hex::encode(Sha256::digest(body));
    let headers = [
        ("host", host),
        ("x-amz-date", amz_date.as_str()),
        ("x-amz-content-sha256", payload_hash.as_str()),
    ];
    let req = SigV4Request {
        method: "PUT",
        uri_path: path,
        query: None,
        headers: &headers,
        payload_hash: &payload_hash,
    };
    let auth = sign_request(
        DEMO_SECRET_KEY,
        DEMO_ACCESS_KEY,
        &req,
        &amz_date,
        &credential_date,
        "us-east-1",
        "s3",
        &["host", "x-amz-date", "x-amz-content-sha256"],
    );
    format!(
        "PUT {path} HTTP/1.1\r\nHost: {host}\r\nx-amz-date: {amz_date}\r\nx-amz-content-sha256: {payload_hash}\r\nAuthorization: {auth}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
}

pub fn demo_handler() -> S3Handler {
    let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
    S3Handler::with_demo(cat)
}
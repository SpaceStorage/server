//! Auth foundation: unbound principal + session token headers (019 T010a/T010b).

mod support;

use support::s3_live::{demo_handler, spawn_s3_handler};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn unbound_principal_refused_before_list_buckets() {
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;

    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let req = "GET / HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nx-spacestorage-unbound-test: 1\r\nConnection: close\r\n\r\n";
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut resp = vec![0u8; 4096];
    let n = stream.read(&mut resp).await.unwrap();
    let text = String::from_utf8_lossy(&resp[..n]);
    assert!(
        text.contains("403") || text.contains("AccessDenied"),
        "unbound principal must not ListBuckets: {text}"
    );
    cancel.cancel();
}

#[tokio::test]
async fn session_token_absent_ok_for_permanent_credential() {
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;

    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let req = "GET / HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n";
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut resp = vec![0u8; 4096];
    let n = stream.read(&mut resp).await.unwrap();
    let text = String::from_utf8_lossy(&resp[..n]);
    assert!(text.contains("200"), "{text}");
    cancel.cancel();
}

#[tokio::test]
async fn session_token_wrong_rejected() {
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;

    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let req = "GET / HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nX-Amz-Security-Token: definitely-wrong\r\nConnection: close\r\n\r\n";
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut resp = vec![0u8; 4096];
    let n = stream.read(&mut resp).await.unwrap();
    let text = String::from_utf8_lossy(&resp[..n]);
    assert!(text.contains("403") || text.contains("InvalidToken"), "{text}");
    cancel.cancel();
}
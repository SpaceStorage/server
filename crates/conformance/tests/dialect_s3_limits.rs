//! Size and connection-cap probes (019 T054). Connection cap is an HTTP refusal
//! after accept (`SlowDown`); a TCP reset is not what the handler does today.

mod support;

use spacestorage_compat::{LimitServices, Limits, SessionCounters};
use support::s3_live::{demo_handler, sha256_verify_http_body, spawn_s3_handler};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

async fn raw(addr: std::net::SocketAddr, req: &str, body: Option<&[u8]>) -> String {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(req.as_bytes()).await.unwrap();
    if let Some(b) = body {
        stream.write_all(b).await.unwrap();
    }
    let mut resp = vec![0u8; 8192];
    let n = stream.read(&mut resp).await.unwrap();
    String::from_utf8_lossy(&resp[..n]).into_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn max_value_and_raised_limit() {
    let mut tight = demo_handler();
    let mut limits = Limits::default();
    assert_eq!(limits.max_value, 16 * 1024 * 1024);
    limits.max_value = 8;
    tight.limit_services = LimitServices::with_fixed(Arc::new(SessionCounters::new()), limits);
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(tight, cancel.clone()).await;
    assert!(raw(
        addr,
        "PUT /lim HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await
    .contains("200"));
    let too_big = raw(
        addr,
        "PUT /lim/k HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 9\r\nConnection: close\r\n\r\n",
        Some(b"012345678"),
    )
    .await;
    assert!(too_big.contains("limit_exceeded"), "{too_big}");
    assert!(too_big.contains("current:9"), "{too_big}");
    cancel.cancel();

    let mut raised = demo_handler();
    let mut limits = Limits::default();
    limits.max_value = 64;
    raised.limit_services = LimitServices::with_fixed(Arc::new(SessionCounters::new()), limits);
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(raised, cancel.clone()).await;
    assert!(raw(
        addr,
        "PUT /raised HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await
    .contains("200"));
    let ok = raw(
        addr,
        "PUT /raised/k HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 32\r\nConnection: close\r\n\r\n",
        Some(&[b'z'; 32]),
    )
    .await;
    assert!(ok.contains("200"), "{ok}");
    let got = raw(
        addr,
        "GET /raised/k HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    sha256_verify_http_body(&[b'z'; 32], &got);
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn principal_connection_cap_refuses_second_request() {
    let mut handler = demo_handler();
    let mut limits = Limits::default();
    limits.max_connections_per_principal = 1;
    handler.limit_services = LimitServices::with_fixed(Arc::new(SessionCounters::new()), limits);
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;

    let mut first = tokio::net::TcpStream::connect(addr).await.unwrap();
    first
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: keep-alive\r\n\r\n")
        .await
        .unwrap();
    let mut buf = [0u8; 1024];
    let n = first.read(&mut buf).await.unwrap();
    let first_text = String::from_utf8_lossy(&buf[..n]);
    assert!(first_text.contains("200"), "{first_text}");

    let second = raw(
        addr,
        "GET / HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(
        second.contains("SlowDown") || second.contains("limit_exceeded") || second.is_empty(),
        "{second}"
    );
    assert!(!second.contains("ListAllMyBucketsResult"), "{second}");
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn max_key_and_multipart_sum_and_raised_fixture() {
    let mut tight = demo_handler();
    let mut limits = Limits::default();
    limits.max_key = 4;
    tight.limit_services = LimitServices::with_fixed(Arc::new(SessionCounters::new()), limits);
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(tight, cancel.clone()).await;
    assert!(raw(
        addr,
        "PUT /klim HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await
    .contains("200"));
    let long_key = raw(
        addr,
        "PUT /klim/toolong HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 1\r\nConnection: close\r\n\r\n",
        Some(b"x"),
    )
    .await;
    assert!(long_key.contains("limit_exceeded"), "{long_key}");
    cancel.cancel();

    let mut parts = demo_handler().with_protocol_config(
        "us-east-1".into(),
        None,
        std::time::Duration::from_secs(24 * 3600),
        1,
        100,
    );
    let mut limits = Limits::default();
    limits.max_value = 10;
    parts.limit_services = LimitServices::with_fixed(Arc::new(SessionCounters::new()), limits);
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(parts, cancel.clone()).await;
    assert!(raw(
        addr,
        "PUT /psum HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await
    .contains("200"));
    let init = raw(
        addr,
        "POST /psum/o?uploads HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    let upload_id = init
        .split("<UploadId>")
        .nth(1)
        .and_then(|r| r.split("</UploadId>").next())
        .unwrap_or("");
    assert!(!upload_id.is_empty(), "{init}");
    for (n, body) in [(1, "123456"), (2, "123456")] {
        let req = format!(
            "PUT /psum/o?uploadId={upload_id}&partNumber={n} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let up = raw(addr, &req, Some(body.as_bytes())).await;
        assert!(up.contains("200"), "{up}");
    }
    let complete = raw(
        addr,
        &format!(
            "POST /psum/o?uploadId={upload_id} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        ),
        None,
    )
    .await;
    assert!(complete.contains("limit_exceeded"), "{complete}");
    cancel.cancel();

    let fixture = include_str!("../../../specs/019-s3-protocol/contracts/fixtures/s3-limits-raised.conf");
    assert!(fixture.contains("max_value 1GiB"), "{fixture}");
    let mut raised = demo_handler();
    let mut limits = Limits::default();
    limits.max_value = 1024 * 1024 * 1024;
    raised.limit_services = LimitServices::with_fixed(Arc::new(SessionCounters::new()), limits);
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(raised, cancel.clone()).await;
    assert!(raw(
        addr,
        "PUT /raisedfix HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await
    .contains("200"));
    let ok = raw(
        addr,
        "PUT /raisedfix/k HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 32\r\nConnection: close\r\n\r\n",
        Some(&[b'a'; 32]),
    )
    .await;
    assert!(ok.contains("200"), "{ok}");
    let got = raw(
        addr,
        "GET /raisedfix/k HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    sha256_verify_http_body(&[b'a'; 32], &got);
    cancel.cancel();
}

//! Unsupported-by-design and not-yet-implemented S3 probes (019 T055).

mod support;

use support::s3_live::{demo_handler, spawn_s3_handler};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

async fn raw(addr: std::net::SocketAddr, req: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut resp = vec![0u8; 8192];
    let n = stream.read(&mut resp).await.unwrap();
    String::from_utf8_lossy(&resp[..n]).into_owned()
}

fn refused(resp: &str) {
    assert!(
        resp.contains("501") || resp.contains("NotImplemented") || resp.contains("compat_must_not"),
        "{resp}"
    );
    assert!(!resp.starts_with("HTTP/1.1 200"), "{resp}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unsupported_and_nyi_never_succeed() {
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let probes = [
        "PUT /b?versioning HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        "GET /b?object-lock HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        "GET /b?legal-hold HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        "GET /b?replication HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        "POST /b?select HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        "GET /b?tagging HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        "GET /b?lifecycle HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        "GET /b?cors HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        "GET /b?website HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        "GET /b?policy HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        "GET /b?acl HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        "PUT /b/k HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nx-amz-server-side-encryption: aws:kms\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        "GET /b/k?attributes HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        "PUT /b/k?uploadId=nope&partNumber=1 HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nx-amz-copy-source: /b/k\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    ];
    for req in probes {
        let resp = raw(addr, req).await;
        refused(&resp);
        if req.contains("attributes") || req.contains("uploadId=nope") {
            assert!(resp.contains("NotImplemented"), "{resp}");
        }
    }
    let sigv2 = raw(
        addr,
        "GET / HTTP/1.1\r\nHost: localhost\r\nAuthorization: AWS demo:c2ln\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(sigv2.contains("InvalidArgument"), "{sigv2}");
    assert!(!sigv2.starts_with("HTTP/1.1 200"), "{sigv2}");

    let canned = raw(
        addr,
        "PUT /b/aclobj HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nx-amz-acl: public-read\r\nContent-Length: 1\r\nConnection: close\r\n\r\nx",
    )
    .await;
    // Canned ACL headers are ignored (documented). They must not open the ACL API.
    assert!(canned.contains("200"), "{canned}");
    let acl = raw(
        addr,
        "GET /b/aclobj?acl HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
    )
    .await;
    refused(&acl);
    cancel.cancel();
}

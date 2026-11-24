//! US5 namespace isolation (019 T050–T053).

mod support;

use spacestorage_authz::Verb;
use spacestorage_handler_s3::{S3Credential, DEMO_SECRET_KEY};
use support::s3_live::{demo_handler, sha256_verify_http_body, spawn_s3_handler};
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

#[tokio::test]
async fn namespaces_do_not_see_each_other() {
    let handler = demo_handler();
    handler.credentials.register(
        "n1",
        S3Credential {
            secret: DEMO_SECRET_KEY.into(),
            namespace: Some("n1".into()),
            session_token: None,
            grants: Verb::IMPLICIT_TENANT.bits(),
            login: "n1".into(),
        },
    );
    handler.credentials.register(
        "n2",
        S3Credential {
            secret: DEMO_SECRET_KEY.into(),
            namespace: Some("n2".into()),
            session_token: None,
            grants: Verb::IMPLICIT_TENANT.bits(),
            login: "n2".into(),
        },
    );
    handler.credentials.register(
        "ro",
        S3Credential {
            secret: DEMO_SECRET_KEY.into(),
            namespace: Some("n1".into()),
            session_token: None,
            grants: Verb::READ.bits(),
            login: "ro".into(),
        },
    );
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;

    assert!(raw(
        addr,
        "PUT /same HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: n1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await
    .contains("200"));
    assert!(raw(
        addr,
        "PUT /same/k HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: n1\r\nContent-Length: 2\r\nConnection: close\r\n\r\n",
        Some(b"n1"),
    )
    .await
    .contains("200"));
    let n1got = raw(
        addr,
        "GET /same/k HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: n1\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    sha256_verify_http_body(b"n1", &n1got);

    let list2 = raw(
        addr,
        "GET / HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: n2\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(!list2.contains("<Name>same</Name>"), "{list2}");

    let cross = raw(
        addr,
        "GET /same/k HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: n2\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(
        cross.contains("AccessDenied") || cross.contains("NoSuchBucket") || cross.contains("404"),
        "{cross}"
    );
    assert!(!cross.contains("n1"), "{cross}");

    let denied = raw(
        addr,
        "PUT /same/x HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: ro\r\nContent-Length: 1\r\nConnection: close\r\n\r\n",
        Some(b"z"),
    )
    .await;
    assert!(denied.contains("AccessDenied"), "{denied}");
    cancel.cancel();
}

#[tokio::test]
async fn temporary_credential_requires_session_token() {
    let handler = demo_handler();
    handler.credentials.register(
        "tmp",
        S3Credential {
            secret: DEMO_SECRET_KEY.into(),
            namespace: Some("demo".into()),
            session_token: Some("session-ok".into()),
            grants: Verb::IMPLICIT_TENANT.bits(),
            login: "tmp".into(),
        },
    );
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let missing = raw(
        addr,
        "GET / HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: tmp\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(missing.contains("MissingToken") || missing.contains("403"), "{missing}");
    let ok = raw(
        addr,
        "GET / HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: tmp\r\nX-Amz-Security-Token: session-ok\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(ok.contains("200"), "{ok}");
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unbound_and_presign_cannot_cross_namespaces() {
    use spacestorage_handler_s3::{amz_now, sign_presigned_query, DEMO_SECRET_KEY};
    let handler = demo_handler();
    handler.credentials.register(
        "n1",
        S3Credential {
            secret: DEMO_SECRET_KEY.into(),
            namespace: Some("n1".into()),
            session_token: None,
            grants: Verb::IMPLICIT_TENANT.bits(),
            login: "n1".into(),
        },
    );
    handler.credentials.register(
        "n2",
        S3Credential {
            secret: DEMO_SECRET_KEY.into(),
            namespace: Some("n2".into()),
            session_token: None,
            grants: Verb::IMPLICIT_TENANT.bits(),
            login: "n2".into(),
        },
    );
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let unbound = raw(
        addr,
        "GET / HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nx-spacestorage-unbound-test: 1\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(unbound.contains("AccessDenied"), "{unbound}");

    assert!(raw(
        addr,
        "PUT /same HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: n2\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await
    .contains("200"));
    assert!(raw(
        addr,
        "PUT /same/secret HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: n2\r\nContent-Length: 4\r\nConnection: close\r\n\r\n",
        Some(b"n2ok"),
    )
    .await
    .contains("200"));
    let secret = raw(
        addr,
        "GET /same/secret HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: n2\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    sha256_verify_http_body(b"n2ok", &secret);
    assert!(raw(
        addr,
        "PUT /same HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: n1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await
    .contains("200"));

    let host = format!("127.0.0.1:{}", addr.port());
    let (amz, date) = amz_now();
    let q = sign_presigned_query(
        DEMO_SECRET_KEY,
        "n1",
        "GET",
        "/same/secret",
        "",
        &host,
        &amz,
        &date,
        "us-east-1",
        120,
        None,
    );
    let cross = raw(
        addr,
        &format!("GET /same/secret?{q} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"),
        None,
    )
    .await;
    assert!(!cross.contains("n2ok"), "{cross}");
    assert!(
        cross.contains("NoSuchKey") || cross.contains("AccessDenied") || cross.contains("404"),
        "{cross}"
    );

    let init = raw(
        addr,
        "POST /same/obj?uploads HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: n1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    let upload_id = init
        .split("<UploadId>")
        .nth(1)
        .and_then(|r| r.split("</UploadId>").next())
        .unwrap_or("")
        .to_string();
    assert!(!upload_id.is_empty(), "{init}");
    let stolen = raw(
        addr,
        &format!(
            "PUT /same/obj?uploadId={upload_id}&partNumber=1 HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: n2\r\nContent-Length: 1\r\nConnection: close\r\n\r\n"
        ),
        Some(b"z"),
    )
    .await;
    assert!(stolen.contains("NoSuchUpload") || stolen.contains("AccessDenied"), "{stolen}");
    cancel.cancel();
}

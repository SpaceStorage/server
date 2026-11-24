//! US2 SigV4 Suite A (019 T029–T032).

mod support;

use support::s3_live::{
    build_signed_put, demo_handler, sha256_record_and_verify, sha256_verify_http_body,
    spawn_s3_handler,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn wrong_signature_rejected() {
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let req = "PUT /s/k HTTP/1.1\r\nHost: localhost\r\nx-amz-date: 20320301T120000Z\r\nx-amz-content-sha256: UNSIGNED-PAYLOAD\r\nAuthorization: AWS4-HMAC-SHA256 Credential=demo/20320301/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=deadbeef\r\nContent-Length: 1\r\nConnection: close\r\n\r\n";
    stream.write_all(req.as_bytes()).await.unwrap();
    stream.write_all(b"x").await.unwrap();
    let mut resp = vec![0u8; 4096];
    let n = stream.read(&mut resp).await.unwrap();
    let text = String::from_utf8_lossy(&resp[..n]);
    assert!(text.contains("SignatureDoesNotMatch"), "{text}");
    cancel.cancel();
}

#[tokio::test]
async fn valid_unsigned_payload_put() {
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let _ = {
        let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
        s.write_all(b"PUT /v HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        s.read(&mut [0u8; 4096]).await.unwrap();
    };
    let host = format!("127.0.0.1:{}", addr.port());
    let body = b"sigv4";
    let req = build_signed_put("/v/o", body, &host);
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(req.as_bytes()).await.unwrap();
    stream.write_all(body).await.unwrap();
    let mut resp = vec![0u8; 4096];
    let n = stream.read(&mut resp).await.unwrap();
    assert!(String::from_utf8_lossy(&resp[..n]).contains("200"));
    let mut get = tokio::net::TcpStream::connect(addr).await.unwrap();
    get.write_all(b"GET /v/o HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut got = vec![0u8; 4096];
    let gn = get.read(&mut got).await.unwrap();
    sha256_verify_http_body(body, &String::from_utf8_lossy(&got[..gn]));
    cancel.cancel();
}

#[tokio::test]
async fn unknown_access_key_rejected() {
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let req = "GET / HTTP/1.1\r\nHost: localhost\r\nx-amz-date: 20320301T120000Z\r\nx-amz-content-sha256: UNSIGNED-PAYLOAD\r\nAuthorization: AWS4-HMAC-SHA256 Credential=not-a-key/20320301/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=deadbeef\r\nConnection: close\r\n\r\n";
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut resp = vec![0u8; 4096];
    let n = stream.read(&mut resp).await.unwrap();
    let text = String::from_utf8_lossy(&resp[..n]);
    assert!(text.contains("InvalidAccessKeyId"), "{text}");
    cancel.cancel();
}

#[tokio::test]
async fn clock_skew_rejected() {
    use spacestorage_handler_s3::{amz_from_unix, sign_request, SigV4Request, DEMO_ACCESS_KEY, DEMO_SECRET_KEY};
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let old = spacestorage_handler_s3::amz_now();
    let skewed = {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        amz_from_unix(now.saturating_sub(3600))
    };
    let _ = old;
    let host = format!("127.0.0.1:{}", addr.port());
    let payload = "UNSIGNED-PAYLOAD";
    let headers = [
        ("host", host.as_str()),
        ("x-amz-date", skewed.0.as_str()),
        ("x-amz-content-sha256", payload),
    ];
    let req = SigV4Request {
        method: "GET",
        uri_path: "/",
        query: None,
        headers: &headers,
        payload_hash: payload,
    };
    let auth = sign_request(
        DEMO_SECRET_KEY,
        DEMO_ACCESS_KEY,
        &req,
        &skewed.0,
        &skewed.1,
        "us-east-1",
        "s3",
        &["host", "x-amz-content-sha256", "x-amz-date"],
    );
    let raw = format!(
        "GET / HTTP/1.1\r\nHost: {host}\r\nx-amz-date: {}\r\nx-amz-content-sha256: {payload}\r\nAuthorization: {auth}\r\nConnection: close\r\n\r\n",
        skewed.0
    );
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(raw.as_bytes()).await.unwrap();
    let mut resp = vec![0u8; 4096];
    let n = stream.read(&mut resp).await.unwrap();
    let text = String::from_utf8_lossy(&resp[..n]);
    assert!(text.contains("RequestTimeTooSkewed"), "{text}");
    cancel.cancel();
}

#[tokio::test]
async fn presigned_get_within_expiry() {
    use spacestorage_handler_s3::{amz_now, sign_presigned_query, DEMO_ACCESS_KEY, DEMO_SECRET_KEY};
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let _ = {
        let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
        s.write_all(b"PUT /p HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let _ = s.read(&mut [0u8; 1024]).await;
        let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
        s.write_all(b"PUT /p/k HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 3\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        s.write_all(b"abc").await.unwrap();
        let _ = s.read(&mut [0u8; 1024]).await;
    };
    let host = format!("127.0.0.1:{}", addr.port());
    let (amz, date) = amz_now();
    let q = sign_presigned_query(
        DEMO_SECRET_KEY,
        DEMO_ACCESS_KEY,
        "GET",
        "/p/k",
        "",
        &host,
        &amz,
        &date,
        "us-east-1",
        120,
        None,
    );
    let raw = format!("GET /p/k?{q} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(raw.as_bytes()).await.unwrap();
    let mut resp = vec![0u8; 4096];
    let n = stream.read(&mut resp).await.unwrap();
    let text = String::from_utf8_lossy(&resp[..n]);
    assert!(text.contains("200"), "{text}");
    assert!(text.contains("abc"), "{text}");
    sha256_verify_http_body(b"abc", &text);
    cancel.cancel();
}

#[tokio::test]
async fn streaming_unsigned_trailer_put() {
    use sha2::{Digest, Sha256};
    use spacestorage_handler_s3::{amz_now, sign_request, SigV4Request, DEMO_ACCESS_KEY, DEMO_SECRET_KEY};
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let _ = {
        let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
        s.write_all(b"PUT /st HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let _ = s.read(&mut [0u8; 512]).await;
    };
    let host = format!("127.0.0.1:{}", addr.port());
    let (amz, date) = amz_now();
    let mode = "STREAMING-UNSIGNED-PAYLOAD-TRAILER";
    let headers = [
        ("host", host.as_str()),
        ("x-amz-date", amz.as_str()),
        ("x-amz-content-sha256", mode),
    ];
    let req = SigV4Request {
        method: "PUT",
        uri_path: "/st/o",
        query: None,
        headers: &headers,
        payload_hash: mode,
    };
    let auth = sign_request(
        DEMO_SECRET_KEY,
        DEMO_ACCESS_KEY,
        &req,
        &amz,
        &date,
        "us-east-1",
        "s3",
        &["host", "x-amz-content-sha256", "x-amz-date"],
    );
    let mut chunk = format!("{:x}\r\n", 5).into_bytes();
    chunk.extend_from_slice(b"hello");
    chunk.extend_from_slice(b"\r\n0\r\n\r\n");
    let raw = format!(
        "PUT /st/o HTTP/1.1\r\nHost: {host}\r\nx-amz-date: {amz}\r\nx-amz-content-sha256: {mode}\r\nAuthorization: {auth}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        chunk.len()
    );
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(raw.as_bytes()).await.unwrap();
    stream.write_all(&chunk).await.unwrap();
    let mut resp = vec![0u8; 4096];
    let n = stream.read(&mut resp).await.unwrap();
    let text = String::from_utf8_lossy(&resp[..n]);
    assert!(text.contains("200"), "{text}");
    let mut get = tokio::net::TcpStream::connect(addr).await.unwrap();
    get.write_all(b"GET /st/o HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let got = String::from_utf8_lossy(&read_all(&mut get).await).into_owned();
    sha256_verify_http_body(b"hello", &got);
    let _ = Sha256::digest(b"hello");
    cancel.cancel();
}

#[tokio::test]
async fn sigv2_and_anonymous_rejected() {
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nAuthorization: AWS demo:c2ln\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut resp = vec![0u8; 4096];
    let n = stream.read(&mut resp).await.unwrap();
    let text = String::from_utf8_lossy(&resp[..n]);
    assert!(text.contains("InvalidArgument"), "{text}");
    assert!(text.contains("x-amz-request-id"), "{text}");

    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let n = stream.read(&mut resp).await.unwrap();
    let text = String::from_utf8_lossy(&resp[..n]);
    assert!(text.contains("AccessDenied"), "{text}");
    cancel.cancel();
}

async fn read_all(stream: &mut tokio::net::TcpStream) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = stream.read(&mut buf).await.unwrap();
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
        if out.len() > 8 * 1024 * 1024 {
            break;
        }
    }
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn presign_expired_is_denied() {
    use spacestorage_handler_s3::{amz_from_unix, sign_presigned_query, DEMO_ACCESS_KEY, DEMO_SECRET_KEY};
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let (amz, date) = amz_from_unix(now.saturating_sub(7200));
    let host = format!("127.0.0.1:{}", addr.port());
    let q = sign_presigned_query(
        DEMO_SECRET_KEY,
        DEMO_ACCESS_KEY,
        "GET",
        "/",
        "",
        &host,
        &amz,
        &date,
        "us-east-1",
        60,
        None,
    );
    let raw = format!("GET /?{q} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(raw.as_bytes()).await.unwrap();
    let resp = read_all(&mut stream).await;
    let text = String::from_utf8_lossy(&resp);
    assert!(text.contains("AccessDenied"), "{text}");
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unsigned_payload_large_put_roundtrip() {
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
    s.write_all(b"PUT /big HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let _ = read_all(&mut s).await;
    let host = format!("127.0.0.1:{}", addr.port());
    let body = vec![b'Q'; 64 * 1024];
    let (amz, date) = spacestorage_handler_s3::amz_now();
    let headers = [
        ("host", host.as_str()),
        ("x-amz-date", amz.as_str()),
        ("x-amz-content-sha256", "UNSIGNED-PAYLOAD"),
    ];
    let signed = spacestorage_handler_s3::SigV4Request {
        method: "PUT",
        uri_path: "/big/o",
        query: None,
        headers: &headers,
        payload_hash: "UNSIGNED-PAYLOAD",
    };
    let auth = spacestorage_handler_s3::sign_request(
        spacestorage_handler_s3::DEMO_SECRET_KEY,
        spacestorage_handler_s3::DEMO_ACCESS_KEY,
        &signed,
        &amz,
        &date,
        "us-east-1",
        "s3",
        &["host", "x-amz-content-sha256", "x-amz-date"],
    );
    let head = format!(
        "PUT /big/o HTTP/1.1\r\nHost: {host}\r\nx-amz-date: {amz}\r\nx-amz-content-sha256: UNSIGNED-PAYLOAD\r\nAuthorization: {auth}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(head.as_bytes()).await.unwrap();
    stream.write_all(&body).await.unwrap();
    let put = String::from_utf8_lossy(&read_all(&mut stream).await).into_owned();
    assert!(put.contains("200"), "{put}");
    let mut get = tokio::net::TcpStream::connect(addr).await.unwrap();
    get.write_all(b"GET /big/o HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let resp = read_all(&mut get).await;
    let text = String::from_utf8_lossy(&resp);
    let start = text.find("\r\n\r\n").map(|i| i + 4).unwrap_or(0);
    sha256_record_and_verify(&body, &resp[start..]);
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn streaming_signed_payload_put_and_upload_part() {
    use spacestorage_handler_s3::{
        amz_now, chunk_string_to_sign, sign_chunk, sign_request, SigV4Request, DEMO_ACCESS_KEY,
        DEMO_SECRET_KEY,
    };
    let handler = demo_handler().with_protocol_config(
        "us-east-1".into(),
        None,
        std::time::Duration::from_secs(24 * 3600),
        1,
        10_000,
    );
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
    s.write_all(b"PUT /st2 HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let _ = read_all(&mut s).await;

    let host = format!("127.0.0.1:{}", addr.port());
    let (amz, date) = amz_now();
    let mode = "STREAMING-AWS4-HMAC-SHA256-PAYLOAD";
    let headers = [
        ("host", host.as_str()),
        ("x-amz-date", amz.as_str()),
        ("x-amz-content-sha256", mode),
    ];
    let req = SigV4Request {
        method: "PUT",
        uri_path: "/st2/o",
        query: None,
        headers: &headers,
        payload_hash: mode,
    };
    let auth = sign_request(
        DEMO_SECRET_KEY,
        DEMO_ACCESS_KEY,
        &req,
        &amz,
        &date,
        "us-east-1",
        "s3",
        &["host", "x-amz-content-sha256", "x-amz-date"],
    );
    let seed = auth.split("Signature=").nth(1).unwrap();
    let scope = format!("{date}/us-east-1/s3/aws4_request");
    let chunk = b"hello";
    let sig1 = sign_chunk(
        DEMO_SECRET_KEY,
        &date,
        "us-east-1",
        "s3",
        &chunk_string_to_sign(&amz, &scope, seed, chunk),
    );
    let sig0 = sign_chunk(
        DEMO_SECRET_KEY,
        &date,
        "us-east-1",
        "s3",
        &chunk_string_to_sign(&amz, &scope, &sig1, b""),
    );
    let mut framed = format!("{len:x};chunk-signature={sig1}\r\n", len = chunk.len()).into_bytes();
    framed.extend_from_slice(chunk);
    framed.extend_from_slice(format!("\r\n0;chunk-signature={sig0}\r\n\r\n").as_bytes());
    let raw = format!(
        "PUT /st2/o HTTP/1.1\r\nHost: {host}\r\nx-amz-date: {amz}\r\nx-amz-content-sha256: {mode}\r\nAuthorization: {auth}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        framed.len()
    );
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(raw.as_bytes()).await.unwrap();
    stream.write_all(&framed).await.unwrap();
    let put = String::from_utf8_lossy(&read_all(&mut stream).await).into_owned();
    assert!(put.contains("200"), "{put}");
    let mut get = tokio::net::TcpStream::connect(addr).await.unwrap();
    get.write_all(b"GET /st2/o HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let got = String::from_utf8_lossy(&read_all(&mut get).await).into_owned();
    assert!(got.ends_with("hello"), "{got}");
    sha256_verify_http_body(b"hello", &got);

    let init = {
        let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
        s.write_all(b"POST /st2/p?uploads HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        String::from_utf8_lossy(&read_all(&mut s).await).into_owned()
    };
    let upload_id = init
        .split("<UploadId>")
        .nth(1)
        .and_then(|r| r.split("</UploadId>").next())
        .unwrap_or("");
    assert!(!upload_id.is_empty(), "{init}");
    let query = format!("partNumber=1&uploadId={upload_id}");
    let (amz, date) = amz_now();
    let headers = [
        ("host", host.as_str()),
        ("x-amz-date", amz.as_str()),
        ("x-amz-content-sha256", mode),
    ];
    let part_req = SigV4Request {
        method: "PUT",
        uri_path: "/st2/p",
        query: Some(&query),
        headers: &headers,
        payload_hash: mode,
    };
    let auth = sign_request(
        DEMO_SECRET_KEY,
        DEMO_ACCESS_KEY,
        &part_req,
        &amz,
        &date,
        "us-east-1",
        "s3",
        &["host", "x-amz-content-sha256", "x-amz-date"],
    );
    let seed = auth.split("Signature=").nth(1).unwrap().to_string();
    let scope = format!("{date}/us-east-1/s3/aws4_request");
    let part = b"world";
    let sig1 = sign_chunk(
        DEMO_SECRET_KEY,
        &date,
        "us-east-1",
        "s3",
        &chunk_string_to_sign(&amz, &scope, &seed, part),
    );
    let sig0 = sign_chunk(
        DEMO_SECRET_KEY,
        &date,
        "us-east-1",
        "s3",
        &chunk_string_to_sign(&amz, &scope, &sig1, b""),
    );
    let mut framed = format!("{len:x};chunk-signature={sig1}\r\n", len = part.len()).into_bytes();
    framed.extend_from_slice(part);
    framed.extend_from_slice(format!("\r\n0;chunk-signature={sig0}\r\n\r\n").as_bytes());
    let raw = format!(
        "PUT /st2/p?{query} HTTP/1.1\r\nHost: {host}\r\nx-amz-date: {amz}\r\nx-amz-content-sha256: {mode}\r\nAuthorization: {auth}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        framed.len()
    );
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(raw.as_bytes()).await.unwrap();
    stream.write_all(&framed).await.unwrap();
    let up = String::from_utf8_lossy(&read_all(&mut stream).await).into_owned();
    assert!(up.contains("200"), "{up}");
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unicode_keys_path_style_and_vhost() {
    use spacestorage_handler_s3::{amz_now, sign_request, SigV4Request, DEMO_ACCESS_KEY, DEMO_SECRET_KEY};
    let key = "a%20b%23%3F%26";
    let handler = demo_handler().with_protocol_config(
        "us-east-1".into(),
        Some("s3.local".into()),
        std::time::Duration::from_secs(24 * 3600),
        5 * 1024 * 1024,
        10_000,
    );
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
    s.write_all(b"PUT /uni HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let _ = read_all(&mut s).await;

    let host = format!("127.0.0.1:{}", addr.port());
    let (amz, date) = amz_now();
    let path = format!("/uni/{key}");
    let headers = [
        ("host", host.as_str()),
        ("x-amz-date", amz.as_str()),
        ("x-amz-content-sha256", "UNSIGNED-PAYLOAD"),
    ];
    let put = SigV4Request {
        method: "PUT",
        uri_path: &path,
        query: None,
        headers: &headers,
        payload_hash: "UNSIGNED-PAYLOAD",
    };
    let auth = sign_request(
        DEMO_SECRET_KEY,
        DEMO_ACCESS_KEY,
        &put,
        &amz,
        &date,
        "us-east-1",
        "s3",
        &["host", "x-amz-content-sha256", "x-amz-date"],
    );
    let raw = format!(
        "PUT {path} HTTP/1.1\r\nHost: {host}\r\nx-amz-date: {amz}\r\nx-amz-content-sha256: UNSIGNED-PAYLOAD\r\nAuthorization: {auth}\r\nContent-Length: 2\r\nConnection: close\r\n\r\n"
    );
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(raw.as_bytes()).await.unwrap();
    stream.write_all(b"ok").await.unwrap();
    let put_resp = String::from_utf8_lossy(&read_all(&mut stream).await).into_owned();
    assert!(put_resp.contains("200"), "{put_resp}");
    let mut path_get = tokio::net::TcpStream::connect(addr).await.unwrap();
    path_get
        .write_all(format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let path_body = String::from_utf8_lossy(&read_all(&mut path_get).await).into_owned();
    sha256_verify_http_body(b"ok", &path_body);

    let (amz, date) = amz_now();
    let query = "list-type=2&prefix=a%20b";
    let headers = [
        ("host", host.as_str()),
        ("x-amz-date", amz.as_str()),
        ("x-amz-content-sha256", "UNSIGNED-PAYLOAD"),
    ];
    let list = SigV4Request {
        method: "GET",
        uri_path: "/uni",
        query: Some(query),
        headers: &headers,
        payload_hash: "UNSIGNED-PAYLOAD",
    };
    let auth = sign_request(
        DEMO_SECRET_KEY,
        DEMO_ACCESS_KEY,
        &list,
        &amz,
        &date,
        "us-east-1",
        "s3",
        &["host", "x-amz-content-sha256", "x-amz-date"],
    );
    let raw = format!(
        "GET /uni?{query} HTTP/1.1\r\nHost: {host}\r\nx-amz-date: {amz}\r\nx-amz-content-sha256: UNSIGNED-PAYLOAD\r\nAuthorization: {auth}\r\nConnection: close\r\n\r\n"
    );
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(raw.as_bytes()).await.unwrap();
    let listed = String::from_utf8_lossy(&read_all(&mut stream).await).into_owned();
    assert!(listed.contains("200"), "{listed}");
    assert!(listed.contains("a%20b") || listed.contains("a b"), "{listed}");

    let vhost = "uni.s3.local";
    let (amz, date) = amz_now();
    let headers = [
        ("host", vhost),
        ("x-amz-date", amz.as_str()),
        ("x-amz-content-sha256", "UNSIGNED-PAYLOAD"),
    ];
    let vpath = format!("/{key}");
    let vput = SigV4Request {
        method: "PUT",
        uri_path: &vpath,
        query: None,
        headers: &headers,
        payload_hash: "UNSIGNED-PAYLOAD",
    };
    let auth = sign_request(
        DEMO_SECRET_KEY,
        DEMO_ACCESS_KEY,
        &vput,
        &amz,
        &date,
        "us-east-1",
        "s3",
        &["host", "x-amz-content-sha256", "x-amz-date"],
    );
    let raw = format!(
        "PUT {vpath} HTTP/1.1\r\nHost: {vhost}\r\nx-amz-date: {amz}\r\nx-amz-content-sha256: UNSIGNED-PAYLOAD\r\nAuthorization: {auth}\r\nContent-Length: 3\r\nConnection: close\r\n\r\n"
    );
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(raw.as_bytes()).await.unwrap();
    stream.write_all(b"vok").await.unwrap();
    let vresp = String::from_utf8_lossy(&read_all(&mut stream).await).into_owned();
    assert!(vresp.contains("200"), "{vresp}");
    let mut get = tokio::net::TcpStream::connect(addr).await.unwrap();
    let raw = format!(
        "GET {vpath} HTTP/1.1\r\nHost: {vhost}\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n"
    );
    get.write_all(raw.as_bytes()).await.unwrap();
    let got = String::from_utf8_lossy(&read_all(&mut get).await).into_owned();
    assert!(got.ends_with("vok"), "{got}");
    sha256_verify_http_body(b"vok", &got);
    cancel.cancel();
}
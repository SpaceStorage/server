//! US1 bucket/object Suite A (019 T018–T019, T018a, T025a).

mod support;

use support::s3_live::{
    build_signed_put, demo_handler, sha256_hex, sha256_record_and_verify, sha256_verify_http_body,
    spawn_s3_handler,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

async fn demo_request(addr: std::net::SocketAddr, req: &str, body: Option<&[u8]>) -> String {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(req.as_bytes()).await.unwrap();
    if let Some(b) = body {
        stream.write_all(b).await.unwrap();
    }
    let mut resp = vec![0u8; 64 * 1024];
    let n = stream.read(&mut resp).await.unwrap();
    String::from_utf8_lossy(&resp[..n]).into_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_buckets_namespace_scoped() {
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let text = demo_request(addr, "GET / HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n", None).await;
    assert!(text.contains("ListAllMyBucketsResult"), "{text}");
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn create_put_get_zero_length_sha256() {
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;

    let _ = demo_request(
        addr,
        "PUT /crud HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;

    let body: &[u8] = b"";
    let host = format!("127.0.0.1:{}", addr.port());
    let req = build_signed_put("/crud/zero", body, &host);
    let put_resp = demo_request(addr, &req, Some(body)).await;
    assert!(put_resp.contains("200"), "{put_resp}");

    let get_resp = demo_request(
        addr,
        "GET /crud/zero HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(get_resp.contains("200"), "{get_resp}");
    let start = get_resp.find("\r\n\r\n").map(|i| i + 4).unwrap_or(0);
    sha256_record_and_verify(body, get_resp[start..].as_bytes());
    let _ = sha256_hex(body);
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn duplicate_create_bucket_error() {
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let mk = "PUT /dup HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    assert!(demo_request(addr, mk, None).await.contains("200"));
    let again = demo_request(addr, mk, None).await;
    assert!(
        again.contains("BucketAlreadyOwnedByYou") || again.contains("BucketAlreadyExists"),
        "{again}"
    );
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn delete_nonempty_bucket_fails() {
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let _ = demo_request(
        addr,
        "PUT /ne HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    let _ = demo_request(
        addr,
        "PUT /ne/obj HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 3\r\nConnection: close\r\n\r\n",
        Some(b"xyz"),
    )
    .await;
    let got = demo_request(
        addr,
        "GET /ne/obj HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    sha256_verify_http_body(b"xyz", &got);
    let del = demo_request(
        addr,
        "DELETE /ne HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(del.contains("BucketNotEmpty"), "{del}");
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn get_bucket_location_constant_region() {
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let _ = demo_request(
        addr,
        "PUT /loc HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    let text = demo_request(
        addr,
        "GET /loc?location HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(text.contains("us-east-1"), "{text}");
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn content_length_matches_body() {
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let _ = demo_request(
        addr,
        "PUT /cl HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    let body = b"0123456789";
    let put = format!(
        "PUT /cl/obj HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    assert!(demo_request(addr, &put, Some(body)).await.contains("200"));
    let get = demo_request(
        addr,
        "GET /cl/obj HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    let declared = get
        .split("\r\n")
        .find(|l| l.to_ascii_lowercase().starts_with("content-length:"))
        .and_then(|l| l.split(':').nth(1))
        .and_then(|n| n.trim().parse::<usize>().ok())
        .expect("content-length");
    let body_start = get.find("\r\n\r\n").map(|i| i + 4).unwrap_or(0);
    let got = get[body_start..].as_bytes();
    assert_eq!(declared, got.len(), "FR-033 Content-Length must match body: {get}");
    assert_eq!(got, body);
    sha256_record_and_verify(body, got);
    assert!(get.contains("text/plain"), "{get}");
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn copy_missing_key_and_metadata_directive() {
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let _ = demo_request(
        addr,
        "PUT /cpy HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    let missing = demo_request(
        addr,
        "PUT /cpy/dest HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nx-amz-copy-source: /cpy/nope\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(missing.contains("NoSuchKey"), "{missing}");

    let body = b"meta-body";
    let put = format!(
        "PUT /cpy/src HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Type: text/plain\r\nx-amz-meta-color: blue\r\nCache-Control: max-age=3\r\nContent-Disposition: attachment\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    assert!(demo_request(addr, &put, Some(body)).await.contains("200"));
    let copied = demo_request(
        addr,
        "PUT /cpy/dst HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nx-amz-copy-source: /cpy/src\r\nx-amz-metadata-directive: COPY\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(copied.contains("200"), "{copied}");
    let head = demo_request(
        addr,
        "HEAD /cpy/dst HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(head.to_ascii_lowercase().contains("x-amz-meta-color: blue"), "{head}");
    assert!(head.to_ascii_lowercase().contains("cache-control: max-age=3"), "{head}");
    assert!(head.to_ascii_lowercase().contains("content-disposition: attachment"), "{head}");
    assert!(head.to_ascii_lowercase().contains("content-encoding: gzip"), "{head}");
    let over = demo_request(
        addr,
        "GET /cpy/dst?response-content-type=application/xml HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(over.to_ascii_lowercase().contains("content-type: application/xml"), "{over}");
    sha256_verify_http_body(body, &over);

    let replaced = demo_request(
        addr,
        "PUT /cpy/dst2 HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nx-amz-copy-source: /cpy/src\r\nx-amz-metadata-directive: REPLACE\r\nContent-Type: application/json\r\nx-amz-meta-color: red\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(replaced.contains("200"), "{replaced}");
    let head2 = demo_request(
        addr,
        "HEAD /cpy/dst2 HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(head2.to_ascii_lowercase().contains("content-type: application/json"), "{head2}");
    assert!(head2.to_ascii_lowercase().contains("x-amz-meta-color: red"), "{head2}");
    let dst2 = demo_request(
        addr,
        "GET /cpy/dst2 HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    sha256_verify_http_body(body, &dst2);
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_max_keys_no_duplicate_or_gap() {
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let _ = demo_request(
        addr,
        "PUT /ls HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    for key in ["a", "b", "c"] {
        let req = format!(
            "PUT /ls/{key} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 1\r\nConnection: close\r\n\r\n"
        );
        assert!(demo_request(addr, &req, Some(b"z")).await.contains("200"));
    }
    let one = demo_request(
        addr,
        "GET /ls/a HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    sha256_verify_http_body(b"z", &one);
    let page = demo_request(
        addr,
        "GET /ls?list-type=2&max-keys=2 HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(page.contains("<IsTruncated>true</IsTruncated>"), "{page}");
    assert!(page.contains("<Key>a</Key>") && page.contains("<Key>b</Key>"), "{page}");
    assert!(!page.contains("<Key>c</Key>"), "{page}");
    let rest = demo_request(
        addr,
        "GET /ls?list-type=2&max-keys=2&continuation-token=b HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(rest.contains("<Key>c</Key>"), "{rest}");
    assert!(!rest.contains("<Key>a</Key>") && !rest.contains("<Key>b</Key>"), "{rest}");
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bucket_type_header_is_reported() {
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let made = demo_request(
        addr,
        "PUT /typed HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nx-spacestorage-type: object_collection\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(made.contains("200"), "{made}");
    let list = demo_request(
        addr,
        "GET / HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(list.contains("object_collection"), "{list}");
    assert!(!list.contains("kv_store"), "{list}");
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn head_bucket_delete_object_and_batch() {
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    assert!(demo_request(
        addr,
        "PUT /hbb HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await
    .contains("200"));
    let head = demo_request(
        addr,
        "HEAD /hbb HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(head.contains("200"), "{head}");

    let body = b"batch-me";
    let put = format!(
        "PUT /hbb/a HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    assert!(demo_request(addr, &put, Some(body)).await.contains("200"));
    let got = demo_request(
        addr,
        "GET /hbb/a HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    let start = got.find("\r\n\r\n").map(|i| i + 4).unwrap_or(0);
    sha256_record_and_verify(body, got[start..].as_bytes());

    let del = demo_request(
        addr,
        "DELETE /hbb/a HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(del.contains("204"), "{del}");
    let missing = demo_request(
        addr,
        "GET /hbb/a HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(missing.contains("NoSuchKey"), "{missing}");

    for key in ["q", "n"] {
        let req = format!(
            "PUT /hbb/{key} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 1\r\nConnection: close\r\n\r\n"
        );
        assert!(demo_request(addr, &req, Some(b"z")).await.contains("200"));
    }
    let qgot = demo_request(
        addr,
        "GET /hbb/q HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    sha256_verify_http_body(b"z", &qgot);
    let ngot = demo_request(
        addr,
        "GET /hbb/n HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    sha256_verify_http_body(b"z", &ngot);
    let quiet_xml = "<Delete><Quiet>true</Quiet><Object><Key>q</Key></Object></Delete>";
    let quiet = format!(
        "POST /hbb?delete HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        quiet_xml.len()
    );
    let quiet_resp = demo_request(addr, &quiet, Some(quiet_xml.as_bytes())).await;
    assert!(quiet_resp.contains("DeleteResult"), "{quiet_resp}");
    assert!(!quiet_resp.contains("<Key>q</Key>"), "{quiet_resp}");

    let loud_xml = "<Delete><Object><Key>n</Key></Object></Delete>";
    let loud = format!(
        "POST /hbb?delete HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        loud_xml.len()
    );
    let loud_resp = demo_request(addr, &loud, Some(loud_xml.as_bytes())).await;
    assert!(loud_resp.contains("<Key>n</Key>"), "{loud_resp}");

    let empty = demo_request(
        addr,
        "DELETE /hbb HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(empty.contains("204"), "{empty}");
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_delimiter_encoding_v1_and_start_after() {
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    assert!(demo_request(
        addr,
        "PUT /lst HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await
    .contains("200"));
    for key in ["p/a", "p/b", "q", "a%20b", "x%26y"] {
        let req = format!(
            "PUT /lst/{key} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 1\r\nConnection: close\r\n\r\n"
        );
        assert!(demo_request(addr, &req, Some(b"z")).await.contains("200"), "{key}");
    }
    let qbody = demo_request(
        addr,
        "GET /lst/q HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    sha256_verify_http_body(b"z", &qbody);

    let delimited = demo_request(
        addr,
        "GET /lst?list-type=2&delimiter=/ HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(delimited.contains("<Prefix>p/</Prefix>"), "{delimited}");
    assert!(delimited.contains("<Key>q</Key>"), "{delimited}");
    assert!(!delimited.contains("<Key>p/a</Key>"), "{delimited}");

    let encoded = demo_request(
        addr,
        "GET /lst?list-type=2&encoding-type=url HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(encoded.contains("<EncodingType>url</EncodingType>"), "{encoded}");
    assert!(encoded.contains("<Key>a%20b</Key>"), "{encoded}");
    assert!(encoded.contains("<Key>x%26y</Key>"), "{encoded}");

    let after = demo_request(
        addr,
        "GET /lst?list-type=2&prefix=p/&start-after=p/a HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(after.contains("<Key>p/b</Key>"), "{after}");
    assert!(!after.contains("<Key>p/a</Key>"), "{after}");

    let v1 = demo_request(
        addr,
        "GET /lst?prefix=p/ HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(v1.contains("<Key>p/a</Key>") && v1.contains("<Key>p/b</Key>"), "{v1}");
    assert!(!v1.contains("NextContinuationToken"), "{v1}");
    let v2 = demo_request(
        addr,
        "GET /lst?list-type=2&prefix=p/ HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(v2.contains("<Key>p/a</Key>") && v2.contains("<Key>p/b</Key>"), "{v2}");
    let marked = demo_request(
        addr,
        "GET /lst?prefix=p/&marker=p/a HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(marked.contains("<Key>p/b</Key>"), "{marked}");
    assert!(!marked.contains("<Key>p/a</Key>"), "{marked}");
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bucket_options_round_trip_without_hiding_type() {
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let made = demo_request(
        addr,
        "PUT /opted HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nx-spacestorage-type: object_collection\r\nx-spacestorage-opt-dims: 768\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(made.contains("200"), "{made}");
    let head = demo_request(
        addr,
        "HEAD /opted HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(
        head.to_ascii_lowercase().contains("x-spacestorage-opt-dims: 768"),
        "{head}"
    );
    let list = demo_request(
        addr,
        "GET / HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(list.contains("object_collection"), "{list}");
    assert!(list.contains("<Name>opted</Name>"), "{list}");
    assert!(!list.contains("kv_store"), "{list}");
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn content_md5_and_checksum_headers() {
    use spacestorage_handler_s3::{
        checksum_crc32_base64, checksum_crc32c_base64, checksum_sha256_base64, content_md5_base64,
    };
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    assert!(demo_request(
        addr,
        "PUT /sum HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await
    .contains("200"));
    let body = b"hello";
    let md5 = content_md5_base64(body);
    let sha = checksum_sha256_base64(body);
    let crc = checksum_crc32_base64(body);
    let crc_c = checksum_crc32c_base64(body);
    let ok = format!(
        "PUT /sum/a HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-MD5: {md5}\r\nx-amz-checksum-sha256: {sha}\r\nx-amz-checksum-crc32: {crc}\r\nx-amz-checksum-crc32c: {crc_c}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let put = demo_request(addr, &ok, Some(body)).await;
    assert!(put.contains("200"), "{put}");
    let summed = demo_request(
        addr,
        "GET /sum/a HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    sha256_verify_http_body(body, &summed);
    let bad = format!(
        "PUT /sum/b HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-MD5: AAAAAAAAAAAAAAAAAAAAAA==\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let mismatch = demo_request(addr, &bad, Some(body)).await;
    assert!(mismatch.contains("BadDigest"), "{mismatch}");
    let bad_crc = format!(
        "PUT /sum/c HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nx-amz-checksum-crc32: AAAAAA==\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let crc_bad = demo_request(addr, &bad_crc, Some(body)).await;
    assert!(crc_bad.contains("BadDigest"), "{crc_bad}");
    cancel.cancel();
}
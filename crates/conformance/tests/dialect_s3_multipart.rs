//! US1 multipart Suite A (019 T020).

mod support;

use support::s3_live::{demo_handler, sha256_record_and_verify, spawn_s3_handler};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

async fn raw(addr: std::net::SocketAddr, req: &str, body: Option<&[u8]>) -> String {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(req.as_bytes()).await.unwrap();
    if let Some(b) = body {
        stream.write_all(b).await.unwrap();
    }
    let mut resp = vec![0u8; 64 * 1024];
    let n = stream.read(&mut resp).await.unwrap();
    String::from_utf8_lossy(&resp[..n]).into_owned()
}

fn extract_upload_id(xml: &str) -> String {
    xml.split("<UploadId>")
        .nth(1)
        .and_then(|r| r.split("</UploadId>").next())
        .unwrap_or("")
        .to_string()
}

fn header_etag(resp: &str) -> String {
    resp.lines()
        .find(|l| l.to_ascii_lowercase().starts_with("etag:"))
        .and_then(|l| l.split(':').nth(1))
        .map(str::trim)
        .unwrap_or("")
        .to_string()
}

#[tokio::test]
async fn multipart_concat_sha256_roundtrip() {
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let _ = raw(
        addr,
        "PUT /mp HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;

    let init = raw(
        addr,
        "POST /mp/obj?uploads HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    let upload_id = extract_upload_id(&init);
    assert!(!upload_id.is_empty(), "{init}");

    let full = b"multipart-bytes";
    let up1 = format!(
        "PUT /mp/obj?uploadId={upload_id}&partNumber=1 HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        full.len()
    );
    assert!(raw(addr, &up1, Some(full)).await.contains("200"));

    let list = raw(
        addr,
        &format!(
            "GET /mp/obj?uploadId={upload_id} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n"
        ),
        None,
    )
    .await;
    assert!(list.contains("ListPartsResult"), "{list}");

    let complete = raw(
        addr,
        &format!(
            "POST /mp/obj?uploadId={upload_id} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        ),
        None,
    )
    .await;
    assert!(complete.contains("200"), "{complete}");

    let get = raw(
        addr,
        "GET /mp/obj HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    let body_start = get.find("\r\n\r\n").map(|i| i + 4).unwrap_or(0);
    let got = get[body_start..].as_bytes();
    let mut expect = Vec::new();
    expect.extend_from_slice(full);
    sha256_record_and_verify(&expect, got);
    cancel.cancel();
}

#[tokio::test]
async fn entity_too_small_intermediate_part() {
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let _ = raw(
        addr,
        "PUT /small HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    let init = raw(
        addr,
        "POST /small/k?uploads HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    let upload_id = extract_upload_id(&init);
    let huge = vec![b'x'; 6 * 1024 * 1024];
    let part1_req = format!(
        "PUT /small/k?uploadId={upload_id}&partNumber=1 HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        huge.len()
    );
    let part1_resp = raw(addr, &part1_req, Some(&huge)).await;
    assert!(part1_resp.contains("200"));
    let e1 = header_etag(&part1_resp);

    let mid = format!(
        "PUT /small/k?uploadId={upload_id}&partNumber=2 HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 100\r\nConnection: close\r\n\r\n"
    );
    let mid_body = vec![b'y'; 100];
    let mid_resp = raw(addr, &mid, Some(&mid_body)).await;
    assert!(
        mid_resp.contains("200"),
        "undersized upload accepted; enforced at complete: {mid_resp}"
    );
    let e2 = header_etag(&mid_resp);

    let tail = format!(
        "PUT /small/k?uploadId={upload_id}&partNumber=3 HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 1\r\nConnection: close\r\n\r\n"
    );
    let tail_resp = raw(addr, &tail, Some(b"z")).await;
    assert!(tail_resp.contains("200"));
    let e3 = header_etag(&tail_resp);

    let xml = format!(
        "<?xml version=\"1.0\"?><CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>{e1}</ETag></Part><Part><PartNumber>2</PartNumber><ETag>{e2}</ETag></Part><Part><PartNumber>3</PartNumber><ETag>{e3}</ETag></Part></CompleteMultipartUpload>"
    );
    let complete = format!(
        "POST /small/k?uploadId={upload_id} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{xml}",
        xml.len()
    );
    let complete_resp = raw(addr, &complete, None).await;
    assert!(
        complete_resp.contains("EntityTooSmall"),
        "{complete_resp}"
    );
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multipart_etag_stable_on_head_and_list() {
    // Single-part ETag is the quoted hex MD5 of the object bytes (FR-020).
    // Multipart ETag is `"<md5-of-raw-part-md5s>-<part-count>"`, not the content MD5.
    let handler = demo_handler().with_protocol_config(
        "us-east-1".into(),
        None,
        std::time::Duration::from_secs(24 * 3600),
        1,
        10_000,
    );
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    assert!(raw(
        addr,
        "PUT /etag HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await
    .contains("200"));
    let init = raw(
        addr,
        "POST /etag/obj?uploads HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    let upload_id = extract_upload_id(&init);
    for (n, body) in [(1u32, "a"), (2, "b")] {
        let req = format!(
            "PUT /etag/obj?uploadId={upload_id}&partNumber={n} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 1\r\nConnection: close\r\n\r\n"
        );
        assert!(raw(addr, &req, Some(body.as_bytes())).await.contains("200"));
    }
    let done = raw(
        addr,
        &format!(
            "POST /etag/obj?uploadId={upload_id} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        ),
        None,
    )
    .await;
    assert!(done.contains("200"), "{done}");
    let head = raw(
        addr,
        "HEAD /etag/obj HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    let etag = head
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("etag:"))
        .unwrap_or("")
        .split(':')
        .nth(1)
        .unwrap_or("")
        .trim()
        .to_string();
    assert!(etag.contains("-2"), "{head}");
    let list = raw(
        addr,
        "GET /etag?list-type=2 HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    let needle = etag.trim_matches('"');
    assert!(list.contains(needle), "{list} missing {etag}");
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multipart_reloads_after_memory_drop() {
    let handler = demo_handler().with_protocol_config(
        "us-east-1".into(),
        None,
        std::time::Duration::from_secs(24 * 3600),
        1,
        10_000,
    );
    let registry = std::sync::Arc::clone(&handler.multipart_registry);
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    assert!(raw(
        addr,
        "PUT /dur HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await
    .contains("200"));
    let init = raw(
        addr,
        "POST /dur/obj?uploads HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    let upload_id = extract_upload_id(&init);
    let part = b"persisted-part";
    let up = format!(
        "PUT /dur/obj?uploadId={upload_id}&partNumber=1 HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        part.len()
    );
    assert!(raw(addr, &up, Some(part)).await.contains("200"));
    registry.write().expect("registry").forget_memory();
    let done = raw(
        addr,
        &format!(
            "POST /dur/obj?uploadId={upload_id} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        ),
        None,
    )
    .await;
    assert!(done.contains("200"), "{done}");
    let get = raw(
        addr,
        "GET /dur/obj HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(get.contains("persisted-part"), "{get}");
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn abort_multipart_upload_removes_inflight() {
    let handler = demo_handler().with_protocol_config(
        "us-east-1".into(),
        None,
        std::time::Duration::from_secs(24 * 3600),
        1,
        10_000,
    );
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    assert!(raw(
        addr,
        "PUT /abort HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await
    .contains("200"));
    let init = raw(
        addr,
        "POST /abort/k?uploads HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    let upload_id = extract_upload_id(&init);
    let up = format!(
        "PUT /abort/k?uploadId={upload_id}&partNumber=1 HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 1\r\nConnection: close\r\n\r\n"
    );
    assert!(raw(addr, &up, Some(b"x")).await.contains("200"));
    let list = raw(
        addr,
        "GET /abort?uploads HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(list.contains(&upload_id), "{list}");
    let abort = raw(
        addr,
        &format!(
            "DELETE /abort/k?uploadId={upload_id} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n"
        ),
        None,
    )
    .await;
    assert!(abort.contains("204") || abort.contains("200"), "{abort}");
    let empty = raw(
        addr,
        "GET /abort?uploads HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(!empty.contains(&upload_id), "{empty}");
    let del_bucket = raw(
        addr,
        "DELETE /abort HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(del_bucket.contains("204") || del_bucket.contains("200"), "{del_bucket}");
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn complete_rejects_invalid_part_order_in_xml() {
    let handler = demo_handler().with_protocol_config(
        "us-east-1".into(),
        None,
        std::time::Duration::from_secs(24 * 3600),
        1,
        10_000,
    );
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let _ = raw(
        addr,
        "PUT /ord HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    let init = raw(
        addr,
        "POST /ord/k?uploads HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    let upload_id = extract_upload_id(&init);
    let mut etags = Vec::new();
    for (n, body) in [(1u32, b"a".as_slice()), (2, b"b".as_slice())] {
        let up = format!(
            "PUT /ord/k?uploadId={upload_id}&partNumber={n} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 1\r\nConnection: close\r\n\r\n"
        );
        let resp = raw(addr, &up, Some(body)).await;
        assert!(resp.contains("200"), "{resp}");
        etags.push(header_etag(&resp));
    }
    let xml = format!(
        r#"<CompleteMultipartUpload><Part><PartNumber>2</PartNumber><ETag>{}</ETag></Part><Part><PartNumber>1</PartNumber><ETag>{}</ETag></Part></CompleteMultipartUpload>"#,
        etags[1], etags[0]
    );
    let bad = format!(
        "POST /ord/k?uploadId={upload_id} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        xml.len()
    );
    let resp = raw(addr, &bad, Some(xml.as_bytes())).await;
    assert!(resp.contains("InvalidPartOrder"), "{resp}");
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multipart_max_parts_boundary() {
    let handler = demo_handler().with_protocol_config(
        "us-east-1".into(),
        None,
        std::time::Duration::from_secs(24 * 3600),
        1,
        2,
    );
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let _ = raw(
        addr,
        "PUT /maxp HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    let init = raw(
        addr,
        "POST /maxp/k?uploads HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    let upload_id = extract_upload_id(&init);
    for n in 1..=2 {
        let up = format!(
            "PUT /maxp/k?uploadId={upload_id}&partNumber={n} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 1\r\nConnection: close\r\n\r\n"
        );
        assert!(raw(addr, &up, Some(b"z")).await.contains("200"));
    }
    let third = format!(
        "PUT /maxp/k?uploadId={upload_id}&partNumber=3 HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 1\r\nConnection: close\r\n\r\n"
    );
    let resp = raw(addr, &third, Some(b"z")).await;
    assert!(resp.contains("InvalidRequest"), "{resp}");
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_and_delete_bucket_see_durable_uploads() {
    let handler = demo_handler().with_protocol_config(
        "us-east-1".into(),
        None,
        std::time::Duration::from_secs(24 * 3600),
        1,
        10_000,
    );
    let registry = std::sync::Arc::clone(&handler.multipart_registry);
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let _ = raw(
        addr,
        "PUT /bdel HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    let init = raw(
        addr,
        "POST /bdel/k?uploads HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    let upload_id = extract_upload_id(&init);
    registry.write().expect("registry").forget_memory();
    let list = raw(
        addr,
        "GET /bdel?uploads HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(list.contains(&upload_id), "{list}");
    let del = raw(
        addr,
        "DELETE /bdel HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(del.contains("BucketNotEmpty"), "{del}");
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_complete_unknown_upload_and_bad_part() {
    let handler = demo_handler().with_protocol_config(
        "us-east-1".into(),
        None,
        std::time::Duration::from_secs(24 * 3600),
        1,
        100,
    );
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    assert!(raw(
        addr,
        "PUT /neg HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await
    .contains("200"));
    let unknown = raw(
        addr,
        "POST /neg/k?uploadId=does-not-exist HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(unknown.contains("NoSuchUpload"), "{unknown}");
    assert!(unknown.contains("404"), "{unknown}");

    let init = raw(
        addr,
        "POST /neg/k?uploads HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    let upload_id = extract_upload_id(&init);
    let up = format!(
        "PUT /neg/k?uploadId={upload_id}&partNumber=1 HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 1\r\nConnection: close\r\n\r\n"
    );
    let up_resp = raw(addr, &up, Some(b"Z")).await;
    assert!(up_resp.contains("200"), "{up_resp}");
    let bad_xml = "<not-complete>";
    let bad = format!(
        "POST /neg/k?uploadId={upload_id} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        bad_xml.len()
    );
    let bad_resp = raw(addr, &bad, Some(bad_xml.as_bytes())).await;
    assert!(bad_resp.contains("MalformedXML"), "{bad_resp}");
    assert!(bad_resp.contains("400"), "{bad_resp}");

    let wrong = format!(
        "POST /neg/k?uploadId={upload_id} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 120\r\nConnection: close\r\n\r\n"
    );
    let xml = r#"<CompleteMultipartUpload><Part><PartNumber>9</PartNumber><ETag>"nope"</ETag></Part></CompleteMultipartUpload>"#;
    let wrong_resp = raw(
        addr,
        &format!(
            "POST /neg/k?uploadId={upload_id} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            xml.len()
        ),
        Some(xml.as_bytes()),
    )
    .await;
    let _ = wrong;
    assert!(wrong_resp.contains("InvalidPart"), "{wrong_resp}");
    cancel.cancel();
}
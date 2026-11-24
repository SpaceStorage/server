//! US3 conditional, range, and response override semantics (019 T037/T041).

mod support;

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
async fn range_and_if_match_and_response_override() {
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    assert!(raw(
        addr,
        "PUT /sem HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await
    .contains("200"));
    let body = b"abcdefghij";
    let put = format!(
        "PUT /sem/o HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    assert!(raw(addr, &put, Some(body)).await.contains("200"));
    let full = raw(
        addr,
        "GET /sem/o HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    sha256_verify_http_body(body, &full);

    let ranged = raw(
        addr,
        "GET /sem/o HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nRange: bytes=0-3\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(ranged.contains("206"), "{ranged}");
    assert!(ranged.ends_with("abcd"), "{ranged}");

    let bad = raw(
        addr,
        "GET /sem/o HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nRange: bytes=50-60\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(bad.contains("416") || bad.contains("InvalidRange"), "{bad}");

    let head = raw(
        addr,
        "HEAD /sem/o HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    let etag = head
        .split("\r\n")
        .find(|l| l.to_ascii_lowercase().starts_with("etag:"))
        .unwrap_or("")
        .split(':')
        .nth(1)
        .unwrap_or("")
        .trim()
        .to_string();
    assert!(!etag.is_empty(), "{head}");
    let miss = raw(
        addr,
        &format!(
            "GET /sem/o HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nIf-Match: \"nope\"\r\nConnection: close\r\n\r\n"
        ),
        None,
    )
    .await;
    assert!(miss.contains("412") || miss.contains("PreconditionFailed"), "{miss}");
    let hit = raw(
        addr,
        &format!(
            "GET /sem/o HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nIf-Match: {etag}\r\nConnection: close\r\n\r\n"
        ),
        None,
    )
    .await;
    assert!(hit.contains("200"), "{hit}");

    let over = raw(
        addr,
        "GET /sem/o?response-content-type=application/json HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(over.to_ascii_lowercase().contains("content-type: application/json"), "{over}");
    let over_more = raw(
        addr,
        "GET /sem/o?response-cache-control=no-cache&response-content-disposition=attachment&response-content-encoding=identity HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await
    .to_ascii_lowercase();
    assert!(over_more.contains("cache-control: no-cache"), "{over_more}");
    assert!(over_more.contains("content-disposition: attachment"), "{over_more}");
    assert!(over_more.contains("content-encoding: identity"), "{over_more}");

    let meta_body = b"meta-body";
    let put_meta = format!(
        "PUT /sem/m HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Type: text/plain\r\nCache-Control: max-age=9\r\nContent-Disposition: attachment; filename=a.txt\r\nContent-Encoding: gzip\r\nx-amz-meta-color: blue\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        meta_body.len()
    );
    assert!(raw(addr, &put_meta, Some(meta_body)).await.contains("200"));
    let meta_got = raw(
        addr,
        "GET /sem/m HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    sha256_verify_http_body(meta_body, &meta_got);
    let head_meta = raw(
        addr,
        "HEAD /sem/m HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await
    .to_ascii_lowercase();
    assert!(head_meta.contains("content-type: text/plain"), "{head_meta}");
    assert!(head_meta.contains("cache-control: max-age=9"), "{head_meta}");
    assert!(head_meta.contains("content-disposition: attachment; filename=a.txt"), "{head_meta}");
    assert!(head_meta.contains("content-encoding: gzip"), "{head_meta}");
    assert!(head_meta.contains("x-amz-meta-color: blue"), "{head_meta}");

    let none = raw(
        addr,
        &format!(
            "GET /sem/o HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nIf-None-Match: {etag}\r\nConnection: close\r\n\r\n"
        ),
        None,
    )
    .await;
    assert!(none.contains("304") || none.contains("NotModified"), "{none}");
    let unmodified = raw(
        addr,
        "GET /sem/o HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nIf-Unmodified-Since: Thu, 01 Jan 1970 00:00:00 GMT\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(unmodified.contains("412") || unmodified.contains("PreconditionFailed"), "{unmodified}");
    let modified = raw(
        addr,
        "GET /sem/o HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nIf-Modified-Since: Wed, 01 Jan 2099 00:00:00 GMT\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(modified.contains("304") || modified.contains("NotModified"), "{modified}");
    let fresh = raw(
        addr,
        "GET /sem/o HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nIf-Modified-Since: Thu, 01 Jan 1970 00:00:00 GMT\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(fresh.contains("200"), "{fresh}");
    cancel.cancel();
}

#[tokio::test]
async fn put_if_none_match_star_blocks_overwrite() {
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let _ = raw(
        addr,
        "PUT /innm HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    let put1 = format!(
        "PUT /innm/k HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        b"first".len()
    );
    assert!(raw(addr, &put1, Some(b"first")).await.contains("200"));
    let put2 = format!(
        "PUT /innm/k HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nIf-None-Match: *\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        b"second".len()
    );
    let blocked = raw(addr, &put2, Some(b"second")).await;
    assert!(
        blocked.contains("412") || blocked.contains("PreconditionFailed"),
        "{blocked}"
    );
    let get = raw(
        addr,
        "GET /innm/k HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(get.contains("first"), "{get}");
    cancel.cancel();
}

fn body_after_headers(resp: &str) -> &str {
    resp.find("\r\n\r\n")
        .map(|i| &resp[i + 4..])
        .unwrap_or("")
}

async fn read_message(stream: &mut tokio::net::TcpStream) -> String {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    loop {
        let n = stream.read(&mut tmp).await.unwrap();
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(header_end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..header_end]).to_ascii_lowercase();
            let mut body = buf[header_end + 4..].to_vec();
            if head.contains("transfer-encoding: chunked") {
                loop {
                    if body.windows(5).any(|w| w == b"0\r\n\r\n") {
                        break;
                    }
                    let n = stream.read(&mut tmp).await.unwrap();
                    if n == 0 {
                        break;
                    }
                    body.extend_from_slice(&tmp[..n]);
                }
            } else if let Some(len) = head
                .lines()
                .find(|l| l.starts_with("content-length:"))
                .and_then(|l| l.split(':').nth(1))
                .and_then(|v| v.trim().parse::<usize>().ok())
            {
                while body.len() < len {
                    let n = stream.read(&mut tmp).await.unwrap();
                    if n == 0 {
                        break;
                    }
                    body.extend_from_slice(&tmp[..n]);
                }
                body.truncate(len);
            }
            let mut all = buf[..header_end + 4].to_vec();
            all.extend_from_slice(&body);
            return String::from_utf8_lossy(&all).into_owned();
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn decode_chunked_payload(body: &str) -> Vec<u8> {
    let bytes = body.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let Some(line_end) = bytes[i..].windows(2).position(|w| w == b"\r\n") else {
            break;
        };
        let line = std::str::from_utf8(&bytes[i..i + line_end]).unwrap_or("");
        let size = usize::from_str_radix(line.split(';').next().unwrap_or("0").trim(), 16).unwrap_or(0);
        i += line_end + 2;
        if size == 0 {
            break;
        }
        if i + size > bytes.len() {
            break;
        }
        out.extend_from_slice(&bytes[i..i + size]);
        i += size + 2;
    }
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn keep_alive_continue_chunked_and_interrupt() {
    use support::s3_live::sha256_record_and_verify;
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    assert!(raw(
        addr,
        "PUT /wire HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await
    .contains("200"));

    let mut ka = tokio::net::TcpStream::connect(addr).await.unwrap();
    ka.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\n\r\n")
        .await
        .unwrap();
    let first = read_message(&mut ka).await;
    assert!(first.contains("200"), "{first}");
    assert!(first.to_ascii_lowercase().contains("keep-alive"), "{first}");
    ka.write_all(b"HEAD /wire HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let second = read_message(&mut ka).await;
    assert!(second.contains("200"), "{second}");

    let mut cont = tokio::net::TcpStream::connect(addr).await.unwrap();
    cont.write_all(b"PUT /wire/c HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 5\r\nExpect: 100-continue\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let interim = read_message(&mut cont).await;
    assert!(interim.contains("100"), "{interim}");
    cont.write_all(b"hello").await.unwrap();
    let put = read_message(&mut cont).await;
    assert!(put.contains("200"), "{put}");

    let chunked = raw(
        addr,
        "PUT /wire/ch HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n0\r\n\r\n",
        None,
    )
    .await;
    assert!(chunked.contains("200"), "{chunked}");
    let got = raw(
        addr,
        "GET /wire/ch HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    sha256_record_and_verify(b"hello", body_after_headers(&got).as_bytes());

    let mut dropped = tokio::net::TcpStream::connect(addr).await.unwrap();
    dropped
        .write_all(b"PUT /wire/partial HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 100\r\nConnection: close\r\n\r\nPARTIAL")
        .await
        .unwrap();
    let _ = dropped.shutdown().await;
    drop(dropped);
    let missing = raw(
        addr,
        "GET /wire/partial HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(missing.contains("NoSuchKey"), "{missing}");
    assert!(missing.contains("<Resource>"), "{missing}");
    assert!(missing.contains("<RequestId>"), "{missing}");

    let unsupported = raw(
        addr,
        "GET /wire?versioning HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    assert!(unsupported.contains("<Code>"), "{unsupported}");
    assert!(unsupported.contains("<Resource>/wire</Resource>") || unsupported.contains("<Resource>"), "{unsupported}");
    assert!(unsupported.contains("<RequestId>"), "{unsupported}");
    assert!(!unsupported.starts_with("HTTP/1.1 200"), "{unsupported}");
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn last_writer_wins_and_delete_put() {
    use support::s3_live::sha256_record_and_verify;
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    assert!(raw(
        addr,
        "PUT /race HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await
    .contains("200"));
    let put = |body: &'static [u8]| async move {
        let req = format!(
            "PUT /race/k HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        raw(addr, &req, Some(body)).await
    };
    let (a, b) = tokio::join!(put(b"AAAA"), put(b"BBBB"));
    assert!(a.contains("200") && b.contains("200"), "{a} {b}");
    let got = raw(
        addr,
        "GET /race/k HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    let bytes = body_after_headers(&got).as_bytes();
    assert!(bytes == b"AAAA" || bytes == b"BBBB", "{got}");
    if bytes == b"AAAA" {
        sha256_record_and_verify(b"AAAA", bytes);
    } else {
        sha256_record_and_verify(b"BBBB", bytes);
    }

    let del = async {
        raw(
            addr,
            "DELETE /race/k HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
            None,
        )
        .await
    };
    let (d, p) = tokio::join!(del, put(b"CCCC"));
    assert!(d.contains("200") || d.contains("204"), "{d}");
    assert!(p.contains("200"), "{p}");
    let after = raw(
        addr,
        "GET /race/k HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n",
        None,
    )
    .await;
    let visible = body_after_headers(&after);
    assert!(
        after.contains("NoSuchKey") || visible.as_bytes() == b"CCCC",
        "{after}"
    );
    if visible.as_bytes() == b"CCCC" {
        sha256_record_and_verify(b"CCCC", visible.as_bytes());
    }
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn streamed_put_roundtrip_is_chunked_response() {
    use support::s3_live::sha256_record_and_verify;
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    assert!(raw(
        addr,
        "PUT /bigw HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        None,
    )
    .await
    .contains("200"));
    let body = vec![b'Z'; spacestorage_handler_s3::CHUNK_WINDOW + 64];
    let mut put = tokio::net::TcpStream::connect(addr).await.unwrap();
    let head = format!(
        "PUT /bigw/o HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    put.write_all(head.as_bytes()).await.unwrap();
    put.write_all(&body).await.unwrap();
    let put_resp = read_message(&mut put).await;
    assert!(put_resp.contains("200"), "{put_resp}");
    let mut get = tokio::net::TcpStream::connect(addr).await.unwrap();
    get.write_all(b"GET /bigw/o HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let resp = read_message(&mut get).await;
    assert!(
        resp.to_ascii_lowercase().contains("transfer-encoding: chunked"),
        "{resp}"
    );
    let payload = decode_chunked_payload(body_after_headers(&resp));
    sha256_record_and_verify(&body, &payload);
    assert!(payload.len() > spacestorage_handler_s3::CHUNK_WINDOW);
    cancel.cancel();
}

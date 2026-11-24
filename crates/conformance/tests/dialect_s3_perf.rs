//! SC-007 streaming proxy and concurrent workers (019 T056 / T073 / T074).
//!
//! Peak RSS on macOS is process-wide and includes the in-memory catalog copy of
//! each stored chunk. The conformance proxy is the pump window
//! (`CHUNK_WINDOW`, 64 KiB): a streamed object is not assembled into one
//! handler `Vec`, and the GET response is chunked. N = 8 concurrent workers.

mod support;

use support::s3_live::{demo_handler, sha256_record_and_verify, spawn_s3_handler};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

const N: usize = 8;

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

fn decode_chunks(body: &str) -> Vec<u8> {
    let bytes = body.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let Some(line_end) = bytes[i..].windows(2).position(|w| w == b"\r\n") else {
            break;
        };
        let line = std::str::from_utf8(&bytes[i..i + line_end]).unwrap_or("0");
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn streamed_sizes_stay_on_the_chunk_window() {
    let handler = demo_handler();
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let mut bucket = tokio::net::TcpStream::connect(addr).await.unwrap();
    bucket
        .write_all(b"PUT /perf HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let _ = read_message(&mut bucket).await;

    let window = spacestorage_handler_s3::CHUNK_WINDOW;
    assert_eq!(window, 64 * 1024, "SC-007 proxy window is fixed");
    for (name, len) in [("a", window + 1), ("b", window * 2 + 8)] {
        let body = vec![b'p'; len];
        let mut put = tokio::net::TcpStream::connect(addr).await.unwrap();
        let head = format!(
            "PUT /perf/{name} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n"
        );
        put.write_all(head.as_bytes()).await.unwrap();
        put.write_all(&body).await.unwrap();
        let put_resp = read_message(&mut put).await;
        assert!(put_resp.contains("200"), "{put_resp}");
        let mut get = tokio::net::TcpStream::connect(addr).await.unwrap();
        let req = format!(
            "GET /perf/{name} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n"
        );
        get.write_all(req.as_bytes()).await.unwrap();
        let resp = read_message(&mut get).await;
        assert!(resp.to_ascii_lowercase().contains("chunked"), "{resp}");
        let start = resp.find("\r\n\r\n").map(|i| i + 4).unwrap_or(0);
        let got = decode_chunks(&resp[start..]);
        sha256_record_and_verify(&body, &got);
        assert_eq!(got.len(), len);
        assert!(got.len() > window);
    }
    cancel.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn eight_workers_sha256_and_multipart_idle() {
    let handler = demo_handler().with_protocol_config(
        "us-east-1".into(),
        None,
        std::time::Duration::from_secs(3600),
        1,
        100,
    );
    let registry = std::sync::Arc::clone(&handler.multipart_registry);
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let mut bucket = tokio::net::TcpStream::connect(addr).await.unwrap();
    bucket
        .write_all(b"PUT /w HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let _ = read_message(&mut bucket).await;

    let mut joins = Vec::new();
    for i in 0..N {
        let body = vec![i as u8; 32];
        joins.push(tokio::spawn(async move {
            let mut put = tokio::net::TcpStream::connect(addr).await.unwrap();
            let head = format!(
                "PUT /w/{i} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            put.write_all(head.as_bytes()).await.unwrap();
            put.write_all(&body).await.unwrap();
            let put_resp = read_message(&mut put).await;
            assert!(put_resp.contains("200"), "{put_resp}");
            let mut get = tokio::net::TcpStream::connect(addr).await.unwrap();
            let req = format!(
                "GET /w/{i} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n"
            );
            get.write_all(req.as_bytes()).await.unwrap();
            let resp = read_message(&mut get).await;
            let start = resp.find("\r\n\r\n").map(|n| n + 4).unwrap_or(0);
            sha256_record_and_verify(&body, resp[start..].as_bytes());
        }));
    }
    for join in joins {
        join.await.unwrap();
    }

    let mut init = tokio::net::TcpStream::connect(addr).await.unwrap();
    init.write_all(b"POST /w/m?uploads HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let created = read_message(&mut init).await;
    let upload_id = created
        .split("<UploadId>")
        .nth(1)
        .and_then(|r| r.split("</UploadId>").next())
        .unwrap_or("")
        .to_string();
    assert!(!upload_id.is_empty(), "{created}");
    registry
        .write()
        .expect("registry")
        .backdate_for_test(&upload_id, std::time::Duration::from_secs(10));
    let mut part = tokio::net::TcpStream::connect(addr).await.unwrap();
    let req = format!(
        "PUT /w/m?uploadId={upload_id}&partNumber=1 HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 1\r\nConnection: close\r\n\r\n"
    );
    part.write_all(req.as_bytes()).await.unwrap();
    part.write_all(b"Q").await.unwrap();
    let part_resp = read_message(&mut part).await;
    assert!(part_resp.contains("200"), "inside idle must not abort: {part_resp}");
    registry
        .write()
        .expect("registry")
        .backdate_for_test(&upload_id, std::time::Duration::from_secs(3601));
    let mut late = tokio::net::TcpStream::connect(addr).await.unwrap();
    let late_req = format!(
        "PUT /w/m?uploadId={upload_id}&partNumber=2 HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 1\r\nConnection: close\r\n\r\n"
    );
    late.write_all(late_req.as_bytes()).await.unwrap();
    late.write_all(b"Q").await.unwrap();
    let late_resp = read_message(&mut late).await;
    assert!(late_resp.contains("NoSuchUpload"), "{late_resp}");
    cancel.cancel();
}

/// M6 smoke: two parts assembled and read back. Elapsed time is recorded, not gated.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multipart_throughput_smoke() {
    let handler = demo_handler().with_protocol_config(
        "us-east-1".into(),
        None,
        std::time::Duration::from_secs(3600),
        1,
        100,
    );
    let cancel = CancellationToken::new();
    let addr = spawn_s3_handler(handler, cancel.clone()).await;
    let mut bucket = tokio::net::TcpStream::connect(addr).await.unwrap();
    bucket
        .write_all(b"PUT /tp HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let _ = read_message(&mut bucket).await;
    let started = std::time::Instant::now();
    let mut init = tokio::net::TcpStream::connect(addr).await.unwrap();
    init.write_all(b"POST /tp/obj?uploads HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let created = read_message(&mut init).await;
    let upload_id = created
        .split("<UploadId>")
        .nth(1)
        .and_then(|r| r.split("</UploadId>").next())
        .unwrap_or("")
        .to_string();
    for (n, bytes) in [(1u32, b"aaaa".as_slice()), (2, b"bbbb".as_slice())] {
        let mut part = tokio::net::TcpStream::connect(addr).await.unwrap();
        let req = format!(
            "PUT /tp/obj?uploadId={upload_id}&partNumber={n} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bytes.len()
        );
        part.write_all(req.as_bytes()).await.unwrap();
        part.write_all(bytes).await.unwrap();
        assert!(read_message(&mut part).await.contains("200"));
    }
    let mut done = tokio::net::TcpStream::connect(addr).await.unwrap();
    let complete = format!(
        "POST /tp/obj?uploadId={upload_id} HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    done.write_all(complete.as_bytes()).await.unwrap();
    assert!(read_message(&mut done).await.contains("200"));
    let mut get = tokio::net::TcpStream::connect(addr).await.unwrap();
    get.write_all(b"GET /tp/obj HTTP/1.1\r\nHost: localhost\r\nx-spacestorage-demo-auth: demo\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let resp = read_message(&mut get).await;
    let start = resp.find("\r\n\r\n").map(|i| i + 4).unwrap_or(0);
    sha256_record_and_verify(b"aaaabbbb", resp[start..].as_bytes());
    let elapsed = started.elapsed();
    assert!(elapsed.as_secs() < 30, "smoke stalled: {elapsed:?}");
    cancel.cancel();
}

//! T072 / T060: HandlersComplete wire-protocol conformance (015).
//!
//! Speaks on-the-wire bytes via handler `serve` loops (duplex or TCP), not dispatch-only.
//! Run: `cargo test -p spacestorage-conformance --features handlers-complete compat_wire`

use bytes::BytesMut;
use spacestorage_compat::{
    ClassifyOutcome, CompatError, DialectProfile, ProtocolId, classify_outcome,
};
use spacestorage_handler_cassandra::CassandraHandler;
use spacestorage_handler_cassandra::wire::{
    Frame, OP_AUTH_RESPONSE, OP_ERROR, OP_OPTIONS, OP_QUERY, OP_READY, OP_RESULT, OP_STARTUP,
    OP_SUPPORTED, VERSION_V4, VERSION_V5, encode_frame, try_decode_frame, write_bytes,
    write_long_string, write_string_map,
};
use spacestorage_handler_clickhouse::{ClickHouseHttpHandler, ClickHouseNativeHandler};
use spacestorage_handler_elasticsearch::ElasticsearchHandler;
use spacestorage_handler_s3::{
    DEMO_ACCESS_KEY, DEMO_SECRET_KEY, S3Handler, SigV4Request, sign_request,
};
use spacestorage_handler_webdav::WebDavHandler;
use spacestorage_types::ContainerCatalog;
use std::sync::{Arc, RwLock};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

const HC: DialectProfile = DialectProfile::HandlersComplete;
const ES_AUTH: &str = "Authorization: Basic ZGVtbzpkZW1v\r\n";
const DAV_AUTH: (&str, &str) = ("Authorization", "Basic ZGVtbzpkZW1v");

fn assert_must_not_classified(protocol: ProtocolId, verb: &str) {
    assert!(
        matches!(
            classify_outcome(HC, protocol, verb),
            ClassifyOutcome::MustNot(_)
        ),
        "{protocol:?} {verb} should be MustNot in HC"
    );
}

// --- Cassandra native client (v4/v5) ---

fn cass_startup_body() -> Vec<u8> {
    let mut body = Vec::new();
    write_string_map(
        &mut body,
        &[("CQL_VERSION", "3.0.0"), ("DRIVER_NAME", "conformance")],
    );
    body
}

fn cass_auth_plain(user: &str, pass: &str) -> Vec<u8> {
    let mut token = Vec::new();
    token.push(0);
    token.extend_from_slice(user.as_bytes());
    token.push(0);
    token.extend_from_slice(pass.as_bytes());
    let mut body = Vec::new();
    write_bytes(&mut body, &token);
    body
}

fn cass_query_body(cql: &str) -> Vec<u8> {
    let mut body = Vec::new();
    write_long_string(&mut body, cql);
    body.extend_from_slice(&0x0001u16.to_be_bytes());
    body
}

async fn cass_read_frame(
    cr: &mut tokio::io::ReadHalf<tokio::io::DuplexStream>,
    acc: &mut BytesMut,
) -> Frame {
    loop {
        if let Ok(Some(f)) = try_decode_frame(acc) {
            return f;
        }
        let mut chunk = [0u8; 4096];
        let n = cr.read(&mut chunk).await.expect("cql read");
        assert!(n > 0, "unexpected eof");
        acc.extend_from_slice(&chunk[..n]);
    }
}

async fn cass_handshake(
    cw: &mut tokio::io::WriteHalf<tokio::io::DuplexStream>,
    cr: &mut tokio::io::ReadHalf<tokio::io::DuplexStream>,
    acc: &mut BytesMut,
    version: u8,
) {
    cw.write_all(&encode_frame(version, 0, OP_OPTIONS, 0, &[]))
        .await
        .unwrap();
    let f = cass_read_frame(cr, acc).await;
    assert_eq!(f.opcode, OP_SUPPORTED);

    cw.write_all(&encode_frame(
        version,
        1,
        OP_STARTUP,
        0,
        &cass_startup_body(),
    ))
    .await
    .unwrap();
    let _auth = cass_read_frame(cr, acc).await;

    cw.write_all(&encode_frame(
        version,
        1,
        OP_AUTH_RESPONSE,
        0,
        &cass_auth_plain("demo", "demo"),
    ))
    .await
    .unwrap();
    let _ = cass_read_frame(cr, acc).await;
    let ready = cass_read_frame(cr, acc).await;
    assert_eq!(ready.opcode, OP_READY);
}

async fn cass_query(
    cw: &mut tokio::io::WriteHalf<tokio::io::DuplexStream>,
    cr: &mut tokio::io::ReadHalf<tokio::io::DuplexStream>,
    acc: &mut BytesMut,
    version: u8,
    stream: i16,
    cql: &str,
) -> Frame {
    cw.write_all(&encode_frame(
        version,
        stream,
        OP_QUERY,
        0,
        &cass_query_body(cql),
    ))
    .await
    .unwrap();
    cass_read_frame(cr, acc).await
}

#[tokio::test]
async fn wire_cassandra_native_v4_must_and_lwt_must_not() {
    assert_must_not_classified(ProtocolId::Cassandra, "LWT");

    let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
    let handler = CassandraHandler::with_demo(Arc::clone(&cat));
    let (client, server) = tokio::io::duplex(65536);
    let (mut cr, mut cw) = tokio::io::split(client);
    let cancel = CancellationToken::new();
    tokio::spawn({
        let c = cancel.clone();
        async move { handler.serve(server, c).await }
    });

    let mut acc = BytesMut::new();
    cass_handshake(&mut cw, &mut cr, &mut acc, VERSION_V4).await;

    let f = cass_query(
        &mut cw,
        &mut cr,
        &mut acc,
        VERSION_V4,
        2,
        "CREATE TABLE wt (id text PRIMARY KEY)",
    )
    .await;
    assert_eq!(f.opcode, OP_RESULT);

    let f = cass_query(
        &mut cw,
        &mut cr,
        &mut acc,
        VERSION_V4,
        3,
        "INSERT INTO wt (id, v) VALUES ('1', 'a') IF NOT EXISTS",
    )
    .await;
    assert_eq!(f.opcode, OP_ERROR);
    let msg = String::from_utf8_lossy(&f.body);
    assert!(
        msg.contains("compat_must_not") || msg.contains("CompatMustNot"),
        "LWT must refuse on wire, got {msg}"
    );

    cancel.cancel();
}

#[tokio::test]
async fn wire_cassandra_native_v5_must_smoke() {
    let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
    let handler = CassandraHandler::with_demo(cat);
    let (client, server) = tokio::io::duplex(65536);
    let (mut cr, mut cw) = tokio::io::split(client);
    let cancel = CancellationToken::new();
    tokio::spawn({
        let c = cancel.clone();
        async move { handler.serve(server, c).await }
    });

    let mut acc = BytesMut::new();
    cass_handshake(&mut cw, &mut cr, &mut acc, VERSION_V5).await;
    let f = cass_query(
        &mut cw,
        &mut cr,
        &mut acc,
        VERSION_V5,
        2,
        "CREATE TABLE v5t (id text PRIMARY KEY)",
    )
    .await;
    assert_eq!(f.opcode, OP_RESULT);
    cancel.cancel();
}

// --- HTTP helpers ---

async fn http_read(client: &mut tokio::io::DuplexStream) -> String {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let n = client.read(&mut tmp).await.expect("http read");
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        let Some(header_end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
            continue;
        };
        let header = String::from_utf8_lossy(&buf[..header_end]);
        let content_length = header
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .map(|v| v.trim().parse::<usize>().ok())
            })
            .flatten()
            .unwrap_or(0);
        let total = header_end + 4 + content_length;
        if buf.len() >= total {
            break;
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn http_request(method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\n");
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    let mut out = req.into_bytes();
    out.extend_from_slice(body);
    out
}

#[tokio::test]
async fn wire_elasticsearch_http_must_search_agg_must_not() {
    assert_must_not_classified(ProtocolId::Elasticsearch, "AGG_TERMS");

    let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
    let handler = ElasticsearchHandler::with_demo(cat);
    let (mut client, server) = tokio::io::duplex(65536);
    let cancel = CancellationToken::new();
    tokio::spawn(async move {
        handler.serve(server, cancel.clone()).await;
    });

    client
        .write_all(
            format!("PUT /docs HTTP/1.1\r\nHost: localhost\r\n{ES_AUTH}Content-Length: 0\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let resp = http_read(&mut client).await;
    assert!(resp.contains("200 OK"), "{resp}");

    client
        .write_all(
            format!(
                "PUT /docs/_doc/1 HTTP/1.1\r\nHost: localhost\r\n{ES_AUTH}Content-Length: 17\r\n\r\n{{\"title\":\"hello\"}}"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let resp = http_read(&mut client).await;
    assert!(resp.contains("created"), "{resp}");

    let search_body = br#"{"query":{"match":{"title":"hello"}}}"#;
    client
        .write_all(
            format!(
                "POST /docs/_search HTTP/1.1\r\nHost: localhost\r\n{ES_AUTH}Content-Length: {}\r\nConnection: keep-alive\r\n\r\n",
                search_body.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    client.write_all(search_body).await.unwrap();
    let resp = http_read(&mut client).await;
    assert!(resp.contains("\"hits\""), "MUST search on wire: {resp}");
    assert!(resp.contains("hello"));

    let agg_body = br#"{"aggs":{"by_title":{"terms":{"field":"title"}}}}"#;
    client
        .write_all(
            format!(
                "POST /docs/_search HTTP/1.1\r\nHost: localhost\r\n{ES_AUTH}Content-Length: {}\r\nConnection: keep-alive\r\n\r\n",
                agg_body.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    client.write_all(agg_body).await.unwrap();
    let resp = http_read(&mut client).await;
    assert!(
        !resp.starts_with("HTTP/1.1 200"),
        "agg MUST NOT return 200 on wire: {resp}"
    );
    assert!(
        resp.contains("agg_not_in_profile") || resp.contains("AggNotInProfile"),
        "{resp}"
    );

    let script_body = br#"{"query":{"script":{"script":"doc['x'].value"}}}"#;
    client
        .write_all(
            format!(
                "POST /docs/_search HTTP/1.1\r\nHost: localhost\r\n{ES_AUTH}Content-Length: {}\r\nConnection: keep-alive\r\n\r\n",
                script_body.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    client.write_all(script_body).await.unwrap();
    let resp = http_read(&mut client).await;
    assert!(
        !resp.starts_with("HTTP/1.1 200"),
        "script query MUST NOT return 200: {resp}"
    );
    assert!(
        resp.contains("compat_must_not") || resp.contains("illegal_argument_exception"),
        "{resp}"
    );

    let unknown_agg = br#"{"aggs":{"x":{"cardinality":{"field":"title"}}}}"#;
    client
        .write_all(
            format!(
                "POST /docs/_search HTTP/1.1\r\nHost: localhost\r\n{ES_AUTH}Content-Length: {}\r\nConnection: close\r\n\r\n",
                unknown_agg.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    client.write_all(unknown_agg).await.unwrap();
    let resp = http_read(&mut client).await;
    assert!(
        !resp.starts_with("HTTP/1.1 200"),
        "unknown agg MUST NOT return 200: {resp}"
    );
    assert!(
        resp.contains("compat_must_not") || resp.contains("illegal_argument_exception"),
        "{resp}"
    );
}

#[tokio::test]
async fn wire_elasticsearch_delete_cat_list_must() {
    let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
    let handler = ElasticsearchHandler::with_demo(cat);
    let (mut client, server) = tokio::io::duplex(65536);
    let cancel = CancellationToken::new();
    tokio::spawn(async move {
        handler.serve(server, cancel).await;
    });

    client
        .write_all(
            format!("PUT /docs HTTP/1.1\r\nHost: localhost\r\n{ES_AUTH}Content-Length: 0\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let resp = http_read(&mut client).await;
    assert!(resp.contains("200 OK"), "{resp}");

    client
        .write_all(
            format!(
                "PUT /docs/_doc/wire-del HTTP/1.1\r\nHost: localhost\r\n{ES_AUTH}Content-Length: 2\r\n\r\n{{}}"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let resp = http_read(&mut client).await;
    assert!(resp.contains("200 OK"), "{resp}");

    client
        .write_all(
            format!(
                "DELETE /docs/_doc/wire-del HTTP/1.1\r\nHost: localhost\r\n{ES_AUTH}Content-Length: 0\r\nConnection: keep-alive\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let resp = http_read(&mut client).await;
    assert!(
        resp.contains("200 OK") && resp.contains("deleted"),
        "DELETE /{{index}}/_doc/{{id}} MUST dispatch: {resp}"
    );
    assert!(
        !resp.contains("index_not_found_exception"),
        "DELETE must not hit catch-all 404: {resp}"
    );

    client
        .write_all(
            format!(
                "GET /_cat HTTP/1.1\r\nHost: localhost\r\n{ES_AUTH}Content-Length: 0\r\nConnection: keep-alive\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let resp = http_read(&mut client).await;
    assert!(
        resp.contains("200 OK"),
        "GET /_cat MUST dispatch CAT: {resp}"
    );
    assert!(
        !resp.contains("index_not_found_exception"),
        "GET /_cat must not hit catch-all 404: {resp}"
    );

    client
        .write_all(
            format!(
                "GET /_cat/indices HTTP/1.1\r\nHost: localhost\r\n{ES_AUTH}Content-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let resp = http_read(&mut client).await;
    assert!(
        resp.contains("200 OK"),
        "GET /_cat/indices MUST dispatch LIST: {resp}"
    );
    assert!(
        !resp.contains("index_not_found_exception"),
        "GET /_cat/indices must not hit catch-all 404: {resp}"
    );
}

#[tokio::test]
async fn wire_elasticsearch_ilm_must_not() {
    assert_must_not_classified(ProtocolId::Elasticsearch, "ILM");

    let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
    let handler = ElasticsearchHandler::with_demo(cat);
    let (mut client, server) = tokio::io::duplex(8192);
    tokio::spawn(async move {
        handler.serve(server, CancellationToken::new()).await;
    });

    client
        .write_all(
            format!(
                "PUT /_ilm/policy/p1 HTTP/1.1\r\nHost: localhost\r\n{ES_AUTH}Content-Length: 2\r\nConnection: close\r\n\r\n{{}}"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let resp = http_read(&mut client).await;
    assert!(
        !resp.contains("200 OK") || resp.contains("compat_must_not"),
        "{resp}"
    );
    assert!(
        resp.contains("compat_must_not") || resp.contains("CompatMustNot") || resp.contains("400"),
        "{resp}"
    );
}

#[tokio::test]
async fn wire_elasticsearch_ingest_must_not() {
    assert_must_not_classified(ProtocolId::Elasticsearch, "INGEST");

    let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
    let handler = ElasticsearchHandler::with_demo(cat);
    let (mut client, server) = tokio::io::duplex(8192);
    tokio::spawn(async move {
        handler.serve(server, CancellationToken::new()).await;
    });

    client
        .write_all(
            format!(
                "PUT /_ingest/pipeline/p1 HTTP/1.1\r\nHost: localhost\r\n{ES_AUTH}Content-Length: 2\r\nConnection: close\r\n\r\n{{}}"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let resp = http_read(&mut client).await;
    assert!(
        !resp.starts_with("HTTP/1.1 200"),
        "ingest MUST NOT return 200: {resp}"
    );
    assert!(
        resp.contains("compat_must_not") || resp.contains("illegal_argument_exception"),
        "{resp}"
    );
}

// --- ClickHouse native + HTTP ---

fn ch_write_var_uint(out: &mut Vec<u8>, mut x: u64) {
    while x >= 0x80 {
        out.push((x as u8) | 0x80);
        x >>= 7;
    }
    out.push(x as u8);
}

fn ch_write_string(out: &mut Vec<u8>, s: &str) {
    ch_write_var_uint(out, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
}

async fn ch_native_hello(client: &mut tokio::io::DuplexStream) {
    let mut hello = Vec::new();
    ch_write_var_uint(&mut hello, 0);
    ch_write_string(&mut hello, "click");
    client.write_all(&hello).await.unwrap();
    let mut resp = vec![0u8; 256];
    let n = client.read(&mut resp).await.unwrap();
    assert!(n > 0);
    assert_eq!(resp[0], 0);
}

async fn ch_native_query(client: &mut tokio::io::DuplexStream, sql: &str) -> String {
    let mut q = Vec::new();
    ch_write_var_uint(&mut q, 1);
    ch_write_string(&mut q, sql);
    client.write_all(&q).await.unwrap();
    let mut resp = vec![0u8; 512];
    let n = tokio::time::timeout(std::time::Duration::from_secs(2), client.read(&mut resp))
        .await
        .expect("ch native query timeout")
        .unwrap();
    assert!(n > 0, "empty reply for {sql}");
    String::from_utf8_lossy(&resp[..n]).into_owned()
}

#[tokio::test]
async fn wire_clickhouse_native_must_and_dictionary_must_not() {
    assert_must_not_classified(ProtocolId::ClickHouse, "DICTIONARY");

    let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
    let handler = ClickHouseNativeHandler::with_demo(cat);
    let (mut client, server) = tokio::io::duplex(8192);
    let cancel = CancellationToken::new();
    tokio::spawn({
        let c = cancel.clone();
        async move { handler.serve(server, c).await }
    });

    ch_native_hello(&mut client).await;
    ch_native_query(&mut client, "CREATE TABLE t (id String)").await;
    ch_native_query(&mut client, "INSERT INTO t VALUES ('1','x')").await;
    ch_native_query(&mut client, "SELECT * FROM t WHERE id = '1'").await;

    let s = ch_native_query(
        &mut client,
        "CREATE DICTIONARY d (id String) PRIMARY KEY id SOURCE(CLICKHOUSE()) LAYOUT(FLAT()) LIFETIME(0)",
    )
    .await;
    assert!(
        s.contains("compat_must_not") || s.contains("CompatMustNot") || s.contains("Code:"),
        "dictionary must refuse: {s}"
    );
    let s = ch_native_query(
        &mut client,
        "CREATE MATERIALIZED VIEW mv ENGINE = MergeTree() AS SELECT 1",
    )
    .await;
    assert!(
        s.contains("compat_must_not") || s.contains("CompatMustNot"),
        "materialized view must refuse: {s}"
    );
    let s = ch_native_query(
        &mut client,
        "CREATE TABLE rep (id String) ENGINE = ReplicatedMergeTree('/clickhouse/tables/rep', '1') ORDER BY id",
    )
    .await;
    assert!(
        s.contains("compat_must_not") || s.contains("CompatMustNot"),
        "replicated engine must refuse: {s}"
    );
    cancel.cancel();
}

#[tokio::test]
async fn wire_clickhouse_http_must_and_dictionary_must_not() {
    assert_must_not_classified(ProtocolId::ClickHouseHttp, "DICTIONARY");

    let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
    let handler = ClickHouseHttpHandler::with_demo(cat);
    let (mut client, server) = tokio::io::duplex(8192);
    let cancel = CancellationToken::new();
    tokio::spawn({
        let c = cancel.clone();
        async move { handler.serve(server, c).await }
    });

    let body = "CREATE TABLE u (id String)";
    client
        .write_all(
            format!(
                "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let resp = http_read(&mut client).await;
    assert!(resp.contains("200") && resp.contains("Ok."), "{resp}");

    let ins = "INSERT INTO u VALUES ('1','y')";
    client
        .write_all(
            format!(
                "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{ins}",
                ins.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let resp = http_read(&mut client).await;
    assert!(resp.contains("Ok."), "{resp}");

    let dict = "CREATE DICTIONARY d (id String) PRIMARY KEY id SOURCE(CLICKHOUSE()) LAYOUT(FLAT()) LIFETIME(0)";
    client
        .write_all(
            format!(
                "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{dict}",
                dict.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let resp = http_read(&mut client).await;
    assert!(
        resp.contains("compat_must_not")
            || resp.contains("CompatMustNot")
            || resp.contains("Exception")
            || resp.contains("400"),
        "{resp}"
    );

    let mv = "CREATE MATERIALIZED VIEW mv ENGINE = MergeTree() AS SELECT 1";
    client
        .write_all(
            format!(
                "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{mv}",
                mv.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let resp = http_read(&mut client).await;
    assert!(
        resp.contains("compat_must_not") || resp.contains("CompatMustNot"),
        "CH HTTP materialized view: {resp}"
    );

    let rep = "CREATE TABLE rep (id String) ENGINE = ReplicatedMergeTree('/clickhouse/tables/rep', '1') ORDER BY id";
    client
        .write_all(
            format!(
                "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{rep}",
                rep.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let resp = http_read(&mut client).await;
    assert!(
        resp.contains("compat_must_not") || resp.contains("CompatMustNot"),
        "CH HTTP replicated: {resp}"
    );
    cancel.cancel();
}

// --- S3 SigV4 over TCP ---

fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(data))
}

fn build_signed_get(path: &str, host: &str) -> String {
    let amz_date = "20220301T120000Z";
    let payload_hash = sha256_hex(b"");
    let headers = [
        ("host", host),
        ("x-amz-date", amz_date),
        ("x-amz-content-sha256", payload_hash.as_str()),
    ];
    let req = SigV4Request {
        method: "GET",
        uri_path: path,
        query: None,
        headers: &headers,
        payload_hash: &payload_hash,
    };
    let auth = sign_request(
        DEMO_SECRET_KEY,
        DEMO_ACCESS_KEY,
        &req,
        amz_date,
        "20220301",
        "us-east-1",
        "s3",
        &["host", "x-amz-date", "x-amz-content-sha256"],
    );
    format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nx-amz-date: {amz_date}\r\nx-amz-content-sha256: {payload_hash}\r\nAuthorization: {auth}\r\nConnection: close\r\n\r\n"
    )
}

fn build_signed_put(path: &str, body: &[u8], host: &str) -> String {
    let amz_date = "20220301T120000Z";
    let payload_hash = "UNSIGNED-PAYLOAD".to_string();
    let headers = [
        ("host", host),
        ("x-amz-date", amz_date),
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
        amz_date,
        "20220301",
        "us-east-1",
        "s3",
        &["host", "x-amz-date", "x-amz-content-sha256"],
    );
    format!(
        "PUT {path} HTTP/1.1\r\nHost: {host}\r\nx-amz-date: {amz_date}\r\nx-amz-content-sha256: {payload_hash}\r\nAuthorization: {auth}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
}

fn build_signed_get_query(path: &str, query: &str, host: &str) -> String {
    let amz_date = "20220301T120000Z";
    let payload_hash = sha256_hex(b"");
    let headers = [
        ("host", host),
        ("x-amz-date", amz_date),
        ("x-amz-content-sha256", payload_hash.as_str()),
    ];
    let req = SigV4Request {
        method: "GET",
        uri_path: path,
        query: Some(query),
        headers: &headers,
        payload_hash: &payload_hash,
    };
    let auth = sign_request(
        DEMO_SECRET_KEY,
        DEMO_ACCESS_KEY,
        &req,
        amz_date,
        "20220301",
        "us-east-1",
        "s3",
        &["host", "x-amz-date", "x-amz-content-sha256"],
    );
    format!(
        "GET {path}?{query} HTTP/1.1\r\nHost: {host}\r\nx-amz-date: {amz_date}\r\nx-amz-content-sha256: {payload_hash}\r\nAuthorization: {auth}\r\nConnection: close\r\n\r\n"
    )
}

fn build_signed_get_versioning(path: &str, host: &str) -> String {
    let amz_date = "20220301T120000Z";
    let payload_hash = sha256_hex(b"");
    let query = "versioning";
    let headers = [
        ("host", host),
        ("x-amz-date", amz_date),
        ("x-amz-content-sha256", payload_hash.as_str()),
    ];
    let req = SigV4Request {
        method: "GET",
        uri_path: path,
        query: Some(query),
        headers: &headers,
        payload_hash: &payload_hash,
    };
    let auth = sign_request(
        DEMO_SECRET_KEY,
        DEMO_ACCESS_KEY,
        &req,
        amz_date,
        "20220301",
        "us-east-1",
        "s3",
        &["host", "x-amz-date", "x-amz-content-sha256"],
    );
    format!(
        "GET {path}?versioning HTTP/1.1\r\nHost: {host}\r\nx-amz-date: {amz_date}\r\nx-amz-content-sha256: {payload_hash}\r\nAuthorization: {auth}\r\nConnection: close\r\n\r\n"
    )
}

async fn spawn_s3_tcp(handler: S3Handler, cancel: CancellationToken) -> std::net::SocketAddr {
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
                    };
                    let c = cancel.child_token();
                    tokio::spawn(async move { h.serve(sock, c).await });
                }
            }
        }
    });
    addr
}

#[tokio::test]
async fn wire_s3_sigv4_must_put_get_versioning_must_not() {
    assert_must_not_classified(ProtocolId::S3, "VERSIONING");

    let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
    let handler = S3Handler::with_demo(Arc::clone(&cat));
    let cancel = CancellationToken::new();
    let addr = spawn_s3_tcp(handler, cancel.clone()).await;
    let host = format!("127.0.0.1:{}", addr.port());

    let mut stream0 = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream0
        .write_all(build_signed_get("/", &host).as_bytes())
        .await
        .unwrap();
    let mut resp0 = vec![0u8; 4096];
    let n0 = stream0.read(&mut resp0).await.unwrap();
    let text0 = String::from_utf8_lossy(&resp0[..n0]);
    assert!(
        text0.contains("200 OK") && text0.contains("ListAllMyBucketsResult"),
        "GET / MUST ListBuckets, not GetObject: {text0}"
    );

    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let req = build_signed_put("/wireb", b"", &host);
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut resp = vec![0u8; 4096];
    let n = stream.read(&mut resp).await.unwrap();
    assert!(String::from_utf8_lossy(&resp[..n]).contains("200 OK"));

    let mut stream2 = tokio::net::TcpStream::connect(addr).await.unwrap();
    let body = b"payload";
    let req2 = build_signed_put("/wireb/key", body, &host);
    stream2.write_all(req2.as_bytes()).await.unwrap();
    stream2.write_all(body).await.unwrap();
    let mut resp2 = vec![0u8; 4096];
    let n2 = stream2.read(&mut resp2).await.unwrap();
    assert!(String::from_utf8_lossy(&resp2[..n2]).contains("200 OK"));

    let mut stream3 = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream3
        .write_all(build_signed_get("/wireb/key", &host).as_bytes())
        .await
        .unwrap();
    let mut resp3 = vec![0u8; 4096];
    let n3 = stream3.read(&mut resp3).await.unwrap();
    let text3 = String::from_utf8_lossy(&resp3[..n3]);
    assert!(text3.contains("200 OK"));
    assert!(text3.contains("payload"));

    let mut stream4 = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream4
        .write_all(build_signed_get_versioning("/wireb", &host).as_bytes())
        .await
        .unwrap();
    let mut resp4 = vec![0u8; 4096];
    let n4 = stream4.read(&mut resp4).await.unwrap();
    let text4 = String::from_utf8_lossy(&resp4[..n4]);
    assert!(
        text4.contains("compat_must_not") || text4.contains("501"),
        "{text4}"
    );

    for (sub, verb) in [
        ("object-lock", "OBJECTLOCK"),
        ("replication", "REPLICATION"),
        ("acl", "ACL"),
    ] {
        assert_must_not_classified(ProtocolId::S3, verb);
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(build_signed_get_query("/wireb", sub, &host).as_bytes())
            .await
            .unwrap();
        let mut resp = vec![0u8; 4096];
        let n = stream.read(&mut resp).await.unwrap();
        let text = String::from_utf8_lossy(&resp[..n]);
        assert!(
            !text.contains("200 OK"),
            "S3 ?{sub} MUST NOT return 200: {text}"
        );
        assert!(
            text.contains("compat_must_not") || text.contains("501"),
            "S3 ?{sub}: {text}"
        );
    }

    cancel.cancel();
}

// --- WebDAV ---

#[tokio::test]
async fn wire_webdav_must_propfind_lock_must_not() {
    assert_must_not_classified(ProtocolId::WebDav, "LOCK");

    let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
    let handler = WebDavHandler::with_demo(cat);
    let (mut client, server) = tokio::io::duplex(65536);
    let cancel = CancellationToken::new();
    tokio::spawn({
        let c = cancel.clone();
        async move { handler.serve(server, c).await }
    });

    client
        .write_all(&http_request("MKCOL", "/w", &[DAV_AUTH], b""))
        .await
        .unwrap();
    let resp = http_read(&mut client).await;
    assert!(resp.contains("200 OK"), "{resp}");

    client
        .write_all(&http_request("PROPFIND", "/", &[DAV_AUTH], b""))
        .await
        .unwrap();
    let resp = http_read(&mut client).await;
    assert!(resp.contains("207 Multi-Status"), "{resp}");

    client
        .write_all(&http_request("LOCK", "/w/f", &[DAV_AUTH], b""))
        .await
        .unwrap();
    let resp = http_read(&mut client).await;
    assert!(
        resp.contains("compat_must_not") || resp.contains("501") || resp.contains("403"),
        "{resp}"
    );
    assert!(
        !resp.contains("200 OK") || resp.contains("compat_must_not"),
        "{resp}"
    );

    cancel.cancel();
}

#[test]
fn matrix_smoke_classify_must_verbs_for_wire_suite() {
    for (protocol, verb) in [
        (ProtocolId::Cassandra, "SELECT"),
        (ProtocolId::Elasticsearch, "SEARCH_MATCH"),
        (ProtocolId::ClickHouse, "SELECT_WHERE"),
        (ProtocolId::ClickHouseHttp, "CREATE"),
        (ProtocolId::S3, "PUTOBJECT"),
        (ProtocolId::WebDav, "PROPFIND"),
    ] {
        assert_eq!(
            classify_outcome(HC, protocol, verb),
            ClassifyOutcome::Must,
            "{protocol:?} {verb}"
        );
    }
    assert!(matches!(
        classify_outcome(HC, ProtocolId::Elasticsearch, "AGG_TERMS"),
        ClassifyOutcome::MustNot(CompatError::AggNotInProfile)
    ));
}

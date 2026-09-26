//! T016 HandlersComplete MUST/MUST NOT smokes (015 SC).
//!
//! Run: `CARGO_TARGET_DIR=target cargo test -p spacestorage-conformance --features handlers-complete`

#![cfg(feature = "handlers-complete")]

use spacestorage_compat::{
    classify_outcome, ClassifyOutcome, CompatError, DialectProfile, ProtocolId,
};
use spacestorage_handler_cassandra::{CassandraReply, SessionState as CassSession, dispatch as cass};
use spacestorage_handler_clickhouse::{
    ClickHouseReply, SessionState as ChSession, dispatch as ch,
};
use spacestorage_handler_elasticsearch::{EsReply, SessionState as EsSession, dispatch as es};
use spacestorage_handler_s3::{S3Reply, SessionState as S3Session, dispatch as s3};
use spacestorage_handler_webdav::{SessionState as DavSession, WebDavReply, dispatch as dav};
use spacestorage_release_profile::{HandlerBuildSet, ReleaseProfile};
use spacestorage_types::ContainerCatalog;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

#[test]
fn hc_profile_requires_remaining_handlers() {
    let set = HandlerBuildSet::for_profile(ReleaseProfile::HandlersComplete);
    for h in [
        "cassandra",
        "elasticsearch",
        "clickhouse",
        "clickhouse-http",
        "s3",
        "webdav",
    ] {
        assert!(set.is_required(h), "{h}");
        assert!(!set.is_forbidden(h), "{h}");
    }
}

#[test]
fn fb_still_forbids_hc_handlers() {
    let set = HandlerBuildSet::for_profile(ReleaseProfile::FirstBinary);
    for h in [
        "cassandra",
        "elasticsearch",
        "clickhouse",
        "clickhouse-http",
        "s3",
        "webdav",
    ] {
        assert!(set.is_forbidden(h), "{h} must stay forbidden in FirstBinary");
    }
}

#[test]
fn cassandra_must_smoke_and_lwt_must_not() {
    let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
    let mut s = CassSession::demo(cat);
    assert!(matches!(cass(&mut s, "CREATE", &["TABLE", "t"]), CassandraReply::Ok));
    assert!(matches!(cass(&mut s, "INSERT", &["t", "1", "a"]), CassandraReply::Ok));
    match cass(&mut s, "SELECT", &["t", "1"]) {
        CassandraReply::Rows(r) => assert_eq!(r[0].1, "a"),
        other => panic!("{other:?}"),
    }
    match cass(&mut s, "LWT", &[]) {
        CassandraReply::Error { code, .. } => assert_eq!(code, "compat_must_not"),
        other => panic!("LWT must refuse, got {other:?}"),
    }
}

#[test]
fn elasticsearch_crud_search_and_agg_ilm_refuse() {
    let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
    let mut s = EsSession::demo(cat);
    assert!(matches!(es(&mut s, "CREATE_INDEX", &["docs"]), EsReply::Ok(_)));
    assert!(matches!(
        es(&mut s, "INDEX", &["docs", "1", r#"{"title":"hello"}"#]),
        EsReply::Ok(_)
    ));
    match es(&mut s, "SEARCH_MATCH", &["docs", "hello"]) {
        EsReply::Ok(v) => assert!(v["hits"]["total"]["value"].as_u64().unwrap() >= 1),
        other => panic!("{other:?}"),
    }
    match es(&mut s, "SEARCH_TERM", &["docs", "hello"]) {
        EsReply::Ok(_) => {}
        other => panic!("{other:?}"),
    }
    // Aggs → agg_not_in_profile; never 200 + empty hits.
    match es(&mut s, "AGG_TERMS", &["docs"]) {
        EsReply::Error { status, body } => {
            assert_eq!(status, 400);
            let reason = body["error"]["reason"].as_str().unwrap_or("");
            assert!(
                reason.contains("agg_not_in_profile") || reason.contains("AggNotInProfile"),
                "{body}"
            );
        }
        EsReply::Ok(_) => panic!("agg MUST NOT return 200"),
    }
    match es(&mut s, "ILM", &[]) {
        EsReply::Error { status, .. } => assert_eq!(status, 400),
        EsReply::Ok(_) => panic!("ILM MUST NOT succeed"),
    }
}

#[test]
fn clickhouse_native_and_http_must_and_dictionary_refuse() {
    let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
    let mut native = ChSession::demo_native(Arc::clone(&cat));
    assert!(matches!(
        ch(&mut native, "CREATE", &["TABLE", "t"]),
        ClickHouseReply::Ok
    ));
    assert!(matches!(
        ch(&mut native, "INSERT", &["t", "1", "x"]),
        ClickHouseReply::Ok
    ));
    match ch(&mut native, "SELECT_WHERE", &["t", "1"]) {
        ClickHouseReply::Rows(r) => assert_eq!(r.len(), 1),
        other => panic!("{other:?}"),
    }

    let mut http = ChSession::demo_http(cat);
    assert!(matches!(
        ch(&mut http, "CREATE", &["TABLE", "u"]),
        ClickHouseReply::Ok
    ));
    match ch(&mut http, "DICTIONARY", &[]) {
        ClickHouseReply::Error { .. } => {}
        other => panic!("dictionary must refuse: {other:?}"),
    }
    match ch(&mut http, "GROUP_BY", &["u"]) {
        ClickHouseReply::Error { .. } => {}
        other => panic!("GROUP BY must refuse in HC: {other:?}"),
    }
}

#[test]
fn s3_must_smoke_and_versioning_refuse() {
    let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
    let mut s = S3Session::demo(cat);
    assert!(matches!(s3(&mut s, "CREATEBUCKET", &["b"]), S3Reply::Ok));
    assert!(matches!(s3(&mut s, "PUTOBJECT", &["b", "k", "v"]), S3Reply::Ok));
    match s3(&mut s, "GETOBJECT", &["b", "k"]) {
        S3Reply::Body(b) => assert_eq!(b, b"v"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(s3(&mut s, "LISTOBJECTSV2", &["b"]), S3Reply::Xml(_)));
    assert!(matches!(s3(&mut s, "DELETEOBJECT", &["b", "k"]), S3Reply::Ok));
    match s3(&mut s, "VERSIONING", &["b"]) {
        S3Reply::Error { code, .. } => assert_eq!(code, "compat_must_not"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn webdav_must_smoke_and_lock_refuse() {
    let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
    let mut s = DavSession::demo(cat);
    assert!(matches!(dav(&mut s, "MKCOL", &["/dir"]), WebDavReply::Ok));
    assert!(matches!(dav(&mut s, "PUT", &["/dir/f", "hi"]), WebDavReply::Ok));
    match dav(&mut s, "GET", &["/dir/f"]) {
        WebDavReply::Body(b) => assert_eq!(b, b"hi"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        dav(&mut s, "PROPFIND", &["/"]),
        WebDavReply::MultiStatus(_)
    ));
    assert!(matches!(
        dav(&mut s, "MOVE", &["/dir/f", "/dir/g"]),
        WebDavReply::Ok
    ));
    assert!(matches!(dav(&mut s, "DELETE", &["/dir/g"]), WebDavReply::Ok));
    match dav(&mut s, "LOCK", &["/x"]) {
        WebDavReply::Error { code, .. } => assert_eq!(code, "compat_must_not"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn classify_must_not_before_ir_named_codes() {
    let p = DialectProfile::HandlersComplete;
    assert_eq!(
        classify_outcome(p, ProtocolId::Elasticsearch, "AGG_TERMS"),
        ClassifyOutcome::MustNot(CompatError::AggNotInProfile)
    );
    assert!(matches!(
        classify_outcome(p, ProtocolId::Cassandra, "LWT"),
        ClassifyOutcome::MustNot(_)
    ));
    assert!(matches!(
        classify_outcome(p, ProtocolId::S3, "VERSIONING"),
        ClassifyOutcome::MustNot(_)
    ));
    assert!(matches!(
        classify_outcome(p, ProtocolId::WebDav, "LOCK"),
        ClassifyOutcome::MustNot(_)
    ));
}

#[test]
fn handshake_mismatch_refuse_under_one_second() {
    // Signature mismatch is owned by 002 protocol-core; here we assert the
    // classify refuse path for a wrong-protocol verb completes quickly.
    let start = Instant::now();
    let _ = classify_outcome(
        DialectProfile::HandlersComplete,
        ProtocolId::Cassandra,
        "GETOBJECT",
    );
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "mismatch classify must finish < 1s"
    );
}

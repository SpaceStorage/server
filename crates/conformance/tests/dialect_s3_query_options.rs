//! FR-009: quorum/timeout headers thread into QueryOptions (019 T016a).

mod support;

use spacestorage_handler_s3::{apply_query_headers, dispatch, SessionState, S3Reply};
use spacestorage_query::options::Sourced;
use spacestorage_types::ContainerCatalog;
use std::sync::{Arc, RwLock};

#[test]
fn quorum_header_sets_session_query_options() {
    let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
    let mut s = SessionState::demo(cat);
    apply_query_headers(&mut s, Some("THREE"), Some("120"));
    assert_eq!(
        s.query_options.quorum_write,
        Sourced::Session("THREE".into())
    );
    assert_eq!(
        s.query_options.timeout,
        Sourced::Session(std::time::Duration::from_secs(120))
    );
}

#[test]
fn dispatch_put_with_quorum_header_applied() {
    let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
    let mut s = SessionState::demo(cat);
    apply_query_headers(&mut s, Some("ONE"), None);
    assert!(matches!(
        dispatch(&mut s, "CREATEBUCKET", &["qop"]),
        S3Reply::Ok
    ));
}
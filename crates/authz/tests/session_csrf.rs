//! Session CSRF mint + stability (018 T009).

use spacestorage_authz::{csrf_tokens_equal, SessionTokenStore};
use std::time::Duration;
use uuid::Uuid;

#[test]
fn csrf_minted_and_stable() {
    let mut store = SessionTokenStore::new();
    let (hex, rec) = store.issue(Uuid::now_v7(), "admin", true, 1, Duration::from_secs(60));
    assert_eq!(rec.csrf_token.len(), 64);
    let again = store.resolve(&hex).expect("resolve");
    assert_eq!(again.csrf_token, rec.csrf_token);
    assert!(csrf_tokens_equal(&rec.csrf_token, &rec.csrf_token));
    assert!(!csrf_tokens_equal(&rec.csrf_token, "00"));
}

#[test]
fn csrf_cleared_on_revoke() {
    let mut store = SessionTokenStore::new();
    let (hex, rec) = store.issue(Uuid::now_v7(), "admin", true, 1, Duration::from_secs(60));
    store.revoke_hash(&rec.token_hash);
    assert!(store.resolve(&hex).is_none());
}
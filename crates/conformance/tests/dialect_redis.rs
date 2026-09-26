//! G5/G5b Redis first-binary dialect.

use spacestorage_handler_redis::{RedisReply, SessionState, dispatch};
use spacestorage_types::ContainerCatalog;
use std::sync::{Arc, RwLock};

#[test]
fn g5_kv_must_list() {
    let catalog = Arc::new(RwLock::new(ContainerCatalog::new()));
    let mut s = SessionState::demo(catalog);
    assert!(matches!(
        dispatch(&mut s, "AUTH", &["demo", "demo"]),
        RedisReply::Ok
    ));
    assert!(matches!(dispatch(&mut s, "PING", &[]), RedisReply::Pong));
    assert!(matches!(
        dispatch(&mut s, "SET", &["k", "v"]),
        RedisReply::Ok
    ));
    assert!(matches!(
        dispatch(&mut s, "GET", &["k"]),
        RedisReply::Bulk(Some(_))
    ));
    assert!(matches!(
        dispatch(&mut s, "EXISTS", &["k"]),
        RedisReply::Integer(1)
    ));
    assert!(matches!(
        dispatch(&mut s, "SCAN", &["0", "COUNT", "10"]),
        RedisReply::Array(_)
    ));
    assert!(matches!(dispatch(&mut s, "SELECT", &["0"]), RedisReply::Ok));
    assert!(matches!(
        dispatch(&mut s, "EXPIRE", &["k", "30"]),
        RedisReply::Integer(1)
    ));
    assert!(matches!(
        dispatch(&mut s, "TTL", &["k"]),
        RedisReply::Integer(_)
    ));
    assert!(matches!(
        dispatch(&mut s, "DEL", &["k"]),
        RedisReply::Integer(1)
    ));
}

#[test]
fn g5b_off_list_verbs_error_never_silent() {
    let catalog = Arc::new(RwLock::new(ContainerCatalog::new()));
    let mut s = SessionState::demo(catalog);
    assert!(matches!(
        dispatch(&mut s, "AUTH", &["demo", "demo"]),
        RedisReply::Ok
    ));
    for cmd in ["HGET", "JSON.GET", "XADD", "ZADD", "LPUSH"] {
        let r = dispatch(&mut s, cmd, &["a", "b"]);
        assert!(
            matches!(r, RedisReply::Error(_)),
            "{cmd} must error, got {r:?}"
        );
    }
}

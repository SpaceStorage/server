//! `017` T020: `WRONGTYPE` is returned on a type-mismatched command and no
//! partial mutation happens (the entry's type and content are unchanged, which
//! is exactly the observable meaning of "`version` unchanged").

use std::sync::{Arc, RwLock};

use spacestorage_compat::DialectProfile;
use spacestorage_handler_redis::{RedisReply, SessionState, dispatch};
use spacestorage_types::ContainerCatalog;

fn complete_session() -> SessionState {
    let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
    let mut s = SessionState::demo(cat);
    s.profile = DialectProfile::CompleteProduct;
    s.authenticated = true;
    s
}

fn int(r: RedisReply) -> i64 {
    match r {
        RedisReply::Integer(i) => i,
        other => panic!("expected integer, got {other:?}"),
    }
}

fn bulk(r: RedisReply) -> Option<Vec<u8>> {
    match r {
        RedisReply::Bulk(b) => b,
        other => panic!("expected bulk, got {other:?}"),
    }
}

fn assert_wrongtype(r: RedisReply) {
    match r {
        RedisReply::Error(e) => assert!(e.starts_with("WRONGTYPE"), "{e}"),
        other => panic!("expected WRONGTYPE, got {other:?}"),
    }
}

#[test]
fn list_command_on_hash_is_wrongtype_and_no_partial_write() {
    let mut s = complete_session();
    assert_eq!(int(dispatch(&mut s, "HSET", &["h", "f", "v"])), 1);

    // LPUSH against a hash must fail and leave the hash untouched.
    assert_wrongtype(dispatch(&mut s, "LPUSH", &["h", "x"]));
    assert_wrongtype(dispatch(&mut s, "RPUSH", &["h", "y"]));
    assert_eq!(int(dispatch(&mut s, "HLEN", &["h"])), 1);
    assert_eq!(bulk(dispatch(&mut s, "HGET", &["h", "f"])), Some(b"v".to_vec()));
}

#[test]
fn hash_command_on_string_is_wrongtype() {
    let mut s = complete_session();
    assert!(matches!(dispatch(&mut s, "SET", &["s", "v"]), RedisReply::Ok));
    assert_wrongtype(dispatch(&mut s, "HGET", &["s", "f"]));
    assert_wrongtype(dispatch(&mut s, "HSET", &["s", "f", "x"]));
    // The string is unchanged.
    assert_eq!(bulk(dispatch(&mut s, "GET", &["s"])), Some(b"v".to_vec()));
}

#[test]
fn get_on_hash_is_wrongtype() {
    let mut s = complete_session();
    assert_eq!(int(dispatch(&mut s, "HSET", &["h", "f", "v"])), 1);
    assert_wrongtype(dispatch(&mut s, "GET", &["h"]));
}

#[test]
fn zset_command_on_list_is_wrongtype_and_no_partial_write() {
    let mut s = complete_session();
    assert_eq!(int(dispatch(&mut s, "RPUSH", &["l", "a", "b"])), 2);
    assert_wrongtype(dispatch(&mut s, "ZADD", &["l", "1", "m"]));
    // List unchanged.
    assert_eq!(int(dispatch(&mut s, "LLEN", &["l"])), 2);
}

#[test]
fn set_command_on_hash_is_wrongtype_and_no_partial_write() {
    let mut s = complete_session();
    assert_eq!(int(dispatch(&mut s, "HSET", &["h", "f", "v"])), 1);
    assert_wrongtype(dispatch(&mut s, "SADD", &["h", "m"]));
    assert_eq!(int(dispatch(&mut s, "HLEN", &["h"])), 1);
}

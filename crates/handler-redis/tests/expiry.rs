//! `017` T021: `TTL`/`PTTL` reporting (`-1` no expiry, `-2` missing) and the
//! hash-field expiry family (HTTL matches the set lifetime; a field read after
//! its deadline is absent while the key remains).

use std::sync::{Arc, RwLock};
use std::thread::sleep;
use std::time::Duration;

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

fn ints(r: RedisReply) -> Vec<i64> {
    match r {
        RedisReply::Array(items) => items
            .into_iter()
            .map(|r| match r {
                RedisReply::Integer(i) => i,
                other => panic!("expected integer element, got {other:?}"),
            })
            .collect(),
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn ttl_reporting_no_expiry_and_missing() {
    let mut s = complete_session();
    assert!(matches!(dispatch(&mut s, "SET", &["k", "v"]), RedisReply::Ok));
    assert_eq!(int(dispatch(&mut s, "TTL", &["k"])), -1);
    assert_eq!(int(dispatch(&mut s, "PTTL", &["k"])), -1);
    // Missing key → -2.
    assert_eq!(int(dispatch(&mut s, "TTL", &["missing"])), -2);
    assert_eq!(int(dispatch(&mut s, "PTTL", &["missing"])), -2);
}

#[test]
fn ttl_reporting_remaining_lifetime() {
    let mut s = complete_session();
    assert!(matches!(dispatch(&mut s, "SET", &["k", "v"]), RedisReply::Ok));
    assert_eq!(int(dispatch(&mut s, "EXPIRE", &["k", "100"])), 1);
    let ttl = int(dispatch(&mut s, "TTL", &["k"]));
    assert!(ttl > 0 && ttl <= 100, "{ttl}");
    let pttl = int(dispatch(&mut s, "PTTL", &["k"]));
    assert!(pttl > 0, "{pttl}");
    // PERSIST clears the deadline.
    assert_eq!(int(dispatch(&mut s, "PERSIST", &["k"])), 1);
    assert_eq!(int(dispatch(&mut s, "TTL", &["k"])), -1);
    assert_eq!(int(dispatch(&mut s, "PERSIST", &["k"])), 0);
    // DEL makes it missing.
    assert_eq!(int(dispatch(&mut s, "DEL", &["k"])), 1);
    assert_eq!(int(dispatch(&mut s, "TTL", &["k"])), -2);
}

#[test]
fn hash_field_expiry_reporting() {
    let mut s = complete_session();
    assert_eq!(int(dispatch(&mut s, "HSET", &["h", "f", "v"])), 1);
    // No field TTL → -1.
    assert_eq!(ints(dispatch(&mut s, "HTTL", &["h", "FIELDS", "1", "f"])), vec![-1]);
    assert_eq!(ints(dispatch(&mut s, "HPTTL", &["h", "FIELDS", "1", "f"])), vec![-1]);
    // Set a 100s field TTL.
    assert_eq!(ints(dispatch(&mut s, "HEXPIRE", &["h", "100", "FIELDS", "1", "f"])), vec![1]);
    let ttl = ints(dispatch(&mut s, "HTTL", &["h", "FIELDS", "1", "f"]));
    assert!(ttl[0] > 0 && ttl[0] <= 100, "{ttl:?}");
    let pttl = ints(dispatch(&mut s, "HPTTL", &["h", "FIELDS", "1", "f"]));
    assert!(pttl[0] > 0, "{pttl:?}");
    let et = ints(dispatch(&mut s, "HEXPIRETIME", &["h", "FIELDS", "1", "f"]));
    assert!(et[0] > 0, "{et:?}");
    // A missing field reports -2.
    assert_eq!(ints(dispatch(&mut s, "HTTL", &["h", "FIELDS", "1", "nope"])), vec![-2]);
}

#[test]
fn field_read_after_deadline_is_absent_key_remains() {
    let mut s = complete_session();
    assert_eq!(int(dispatch(&mut s, "HSET", &["h", "f", "v"])), 1);
    // 50 ms field TTL, then wait past it.
    assert_eq!(ints(dispatch(&mut s, "HPEXPIRE", &["h", "50", "FIELDS", "1", "f"])), vec![1]);
    sleep(Duration::from_millis(150));
    assert!(matches!(dispatch(&mut s, "HGET", &["h", "f"]), RedisReply::Bulk(None)));
    assert_eq!(int(dispatch(&mut s, "HEXISTS", &["h", "f"])), 0);
    assert_eq!(int(dispatch(&mut s, "HLEN", &["h"])), 0);
    // The key itself is still present (only the field expired).
    assert_eq!(int(dispatch(&mut s, "EXISTS", &["h"])), 1);
}

//! `017` T043: transaction-session invariants (data-model §6).
//!
//! In-process unit tests over `dispatch` for the complete-product profile: a
//! queuing-time arity/syntax error flags `dirty`; `EXEC` on a dirty session
//! returns `-EXECABORT` and runs nothing; blocking commands, `SUBSCRIBE`, and a
//! nested `MULTI` in the queue are queuing errors; `WATCH` outside `MULTI` is
//! held until `EXEC`, `DISCARD`, `UNWATCH`, or disconnect.

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

fn is_queued(r: &RedisReply) -> bool {
    matches!(r, RedisReply::Simple(s) if s == "QUEUED")
}

#[test]
fn transactions_arity_error_flags_dirty_and_execaborts() {
    let mut s = complete_session();
    assert!(matches!(dispatch(&mut s, "MULTI", &[]), RedisReply::Ok));
    assert!(is_queued(&dispatch(&mut s, "SET", &["a", "1"])));

    // `GET` with no key is an obvious arity error at queue time.
    match dispatch(&mut s, "GET", &[]) {
        RedisReply::Error(e) => assert!(e.contains("wrong number of arguments"), "{e}"),
        other => panic!("expected queuing arity error, got {other:?}"),
    }
    assert!(s.txn.is_dirty(), "arity error must flag dirty");

    match dispatch(&mut s, "EXEC", &[]) {
        RedisReply::Error(e) => assert!(e.starts_with("EXECABORT"), "{e}"),
        other => panic!("expected EXECABORT, got {other:?}"),
    }
    // Nothing ran: the queued SET never applied.
    assert!(matches!(dispatch(&mut s, "GET", &["a"]), RedisReply::Bulk(None)));
    assert!(s.txn.is_idle(), "session returns to Idle after EXECABORT");
}

#[test]
fn transactions_nested_multi_is_a_queuing_error() {
    let mut s = complete_session();
    assert!(matches!(dispatch(&mut s, "MULTI", &[]), RedisReply::Ok));
    match dispatch(&mut s, "MULTI", &[]) {
        RedisReply::Error(e) => assert!(e.contains("can not be nested"), "{e}"),
        other => panic!("expected nested-MULTI error, got {other:?}"),
    }
    assert!(s.txn.is_dirty());
    match dispatch(&mut s, "EXEC", &[]) {
        RedisReply::Error(e) => assert!(e.starts_with("EXECABORT"), "{e}"),
        other => panic!("expected EXECABORT, got {other:?}"),
    }
}

#[test]
fn transactions_blocking_and_subscribe_are_queuing_errors() {
    let cases: &[(&str, &[&str])] = &[
        ("BLPOP", &["k", "0"]),
        ("BRPOP", &["k", "0"]),
        ("BRPOPLPUSH", &["a", "b", "0"]),
        ("BLMOVE", &["a", "b", "LEFT", "RIGHT", "0"]),
        ("BZPOPMIN", &["z", "0"]),
        ("BZPOPMAX", &["z", "0"]),
        ("XREAD", &["BLOCK", "0", "STREAMS", "s", "$"]),
        ("XREADGROUP", &["GROUP", "g", "c", "BLOCK", "0", "STREAMS", "s", ">"]),
        ("SUBSCRIBE", &["ch"]),
        ("PSUBSCRIBE", &["p*"]),
        ("SSUBSCRIBE", &["ch"]),
        ("UNSUBSCRIBE", &["ch"]),
    ];
    for (cmd, args) in cases {
        let mut s = complete_session();
        assert!(matches!(dispatch(&mut s, "MULTI", &[]), RedisReply::Ok));
        match dispatch(&mut s, cmd, args) {
            RedisReply::Error(_) => {}
            other => panic!("{cmd} should be a queuing error, got {other:?}"),
        }
        assert!(s.txn.is_dirty(), "{cmd} must flag dirty");
        match dispatch(&mut s, "EXEC", &[]) {
            RedisReply::Error(e) => assert!(e.starts_with("EXECABORT"), "{cmd}: {e}"),
            other => panic!("{cmd}: expected EXECABORT, got {other:?}"),
        }
    }
}

#[test]
fn transactions_watch_is_held_until_exec_discard_or_unwatch() {
    let mut s = complete_session();

    // WATCH outside MULTI registers but does not enter a transaction.
    assert!(matches!(dispatch(&mut s, "WATCH", &["k"]), RedisReply::Ok));
    assert!(s.txn.is_idle());
    assert_eq!(s.txn.watched.len(), 1);

    // MULTI does not clear watched; DISCARD clears both queue and watched.
    assert!(matches!(dispatch(&mut s, "MULTI", &[]), RedisReply::Ok));
    assert!(is_queued(&dispatch(&mut s, "SET", &["k", "1"])));
    assert!(matches!(dispatch(&mut s, "DISCARD", &[]), RedisReply::Ok));
    assert_eq!(s.txn.watched.len(), 0, "DISCARD releases WATCH");
    assert!(matches!(dispatch(&mut s, "GET", &["k"]), RedisReply::Bulk(None)));

    // WATCH then a completed EXEC clears watched.
    assert!(matches!(dispatch(&mut s, "WATCH", &["k"]), RedisReply::Ok));
    assert!(matches!(dispatch(&mut s, "MULTI", &[]), RedisReply::Ok));
    assert!(is_queued(&dispatch(&mut s, "SET", &["k", "1"])));
    assert!(matches!(dispatch(&mut s, "EXEC", &[]), RedisReply::Array(_)));
    assert_eq!(s.txn.watched.len(), 0, "EXEC releases WATCH");

    // UNWATCH clears watched at any time.
    assert!(matches!(dispatch(&mut s, "WATCH", &["k"]), RedisReply::Ok));
    assert!(matches!(dispatch(&mut s, "UNWATCH", &[]), RedisReply::Ok));
    assert_eq!(s.txn.watched.len(), 0, "UNWATCH releases WATCH");
}

#[test]
fn transactions_exec_and_discard_without_multi_error() {
    let mut s = complete_session();
    match dispatch(&mut s, "EXEC", &[]) {
        RedisReply::Error(e) => assert!(e.contains("EXEC without MULTI"), "{e}"),
        other => panic!("expected EXEC-without-MULTI error, got {other:?}"),
    }
    match dispatch(&mut s, "DISCARD", &[]) {
        RedisReply::Error(e) => assert!(e.contains("DISCARD without MULTI"), "{e}"),
        other => panic!("expected DISCARD-without-MULTI error, got {other:?}"),
    }
}

#[test]
fn transactions_watch_inside_multi_is_a_queuing_error() {
    let mut s = complete_session();
    assert!(matches!(dispatch(&mut s, "MULTI", &[]), RedisReply::Ok));
    match dispatch(&mut s, "WATCH", &["k"]) {
        RedisReply::Error(e) => assert!(e.contains("WATCH inside MULTI"), "{e}"),
        other => panic!("expected WATCH-inside-MULTI error, got {other:?}"),
    }
    assert!(s.txn.is_dirty());
}

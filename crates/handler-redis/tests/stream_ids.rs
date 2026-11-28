//! `017` T033: stream-ID invariants (data-model §4) and the total-order
//! leadership-lease epoch fence at the handler seam.
//!
//! These are in-process unit tests over `dispatch` so they can drive the
//! `lease` fence directly: `*` IDs strictly increase, `0-*` allocates the next
//! sequence, an explicit ID ≤ `last_id` is rejected and appends nothing, a
//! stale total-order epoch is refused and stores nothing, and with no lease
//! holder (`non_granting`) the append writes nothing and does **not** fall open
//! into producer order.

use std::sync::{Arc, RwLock};

use spacestorage_compat::DialectProfile;
use spacestorage_handler_redis::{RedisReply, SessionState, StreamLeaseAuthority, dispatch};
use spacestorage_types::ContainerCatalog;

fn complete_session() -> SessionState {
    let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
    let mut s = SessionState::demo(cat);
    s.profile = DialectProfile::CompleteProduct;
    s.authenticated = true;
    s
}

fn bulk(r: RedisReply) -> Vec<u8> {
    match r {
        RedisReply::Bulk(Some(b)) => b,
        other => panic!("expected bulk, got {other:?}"),
    }
}

fn int(r: RedisReply) -> i64 {
    match r {
        RedisReply::Integer(i) => i,
        other => panic!("expected integer, got {other:?}"),
    }
}

fn parse_id(bytes: &[u8]) -> (u64, u64) {
    let t = std::str::from_utf8(bytes).expect("id is utf8");
    let (ms, seq) = t.split_once('-').expect("id is ms-seq");
    (ms.parse().expect("ms"), seq.parse().expect("seq"))
}

/// First `(id, value)` pair of an `XRANGE key - +` reply.
fn first_entry(reply: RedisReply) -> (String, String) {
    match reply {
        RedisReply::Array(items) => {
            let RedisReply::Array(entry) = &items[0] else {
                panic!("entry not array: {items:?}");
            };
            let RedisReply::Bulk(Some(id)) = &entry[0] else {
                panic!("id not bulk: {entry:?}");
            };
            let RedisReply::Array(fields) = &entry[1] else {
                panic!("fields not array: {entry:?}");
            };
            let RedisReply::Bulk(Some(value)) = &fields[1] else {
                panic!("value not bulk: {fields:?}");
            };
            (
                String::from_utf8(id.clone()).unwrap(),
                String::from_utf8(value.clone()).unwrap(),
            )
        }
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn star_ids_strictly_increase() {
    let mut s = complete_session();
    let a = bulk(dispatch(&mut s, "XADD", &["s", "*", "f", "a"]));
    let b = bulk(dispatch(&mut s, "XADD", &["s", "*", "f", "b"]));
    let c = bulk(dispatch(&mut s, "XADD", &["s", "*", "f", "c"]));
    let (am, asq) = parse_id(&a);
    let (bm, bsq) = parse_id(&b);
    let (cm, csq) = parse_id(&c);
    assert!((bm, bsq) > (am, asq), "b={b:?} must exceed a={a:?}");
    assert!((cm, csq) > (bm, bsq), "c={c:?} must exceed b={b:?}");
    assert_eq!(int(dispatch(&mut s, "XLEN", &["s"])), 3);
}

#[test]
fn autoseq_allocates_next_sequence() {
    let mut s = complete_session();
    // Seed the stream at ms 0 so `0-*` is a legal (greater) id.
    assert_eq!(bulk(dispatch(&mut s, "XADD", &["s", "0-1", "f", "v"])), b"0-1");
    assert_eq!(bulk(dispatch(&mut s, "XADD", &["s", "0-*", "f", "v"])), b"0-2");
    assert_eq!(bulk(dispatch(&mut s, "XADD", &["s", "0-*", "f", "v"])), b"0-3");
}

#[test]
fn explicit_id_not_greater_than_last_is_rejected_and_appends_nothing() {
    let mut s = complete_session();
    assert_eq!(bulk(dispatch(&mut s, "XADD", &["s", "5-5", "f", "v"])), b"5-5");
    assert_eq!(int(dispatch(&mut s, "XLEN", &["s"])), 1);

    // Equal to `last_id` → rejected.
    match dispatch(&mut s, "XADD", &["s", "5-5", "f", "clobber"]) {
        RedisReply::Error(e) => assert!(e.contains("equal or smaller"), "{e}"),
        other => panic!("expected rejection, got {other:?}"),
    }
    // Strictly smaller ms → rejected.
    match dispatch(&mut s, "XADD", &["s", "4-9", "f", "clobber"]) {
        RedisReply::Error(e) => assert!(e.contains("equal or smaller"), "{e}"),
        other => panic!("expected rejection, got {other:?}"),
    }

    // Nothing was appended and the surviving body is unchanged.
    assert_eq!(int(dispatch(&mut s, "XLEN", &["s"])), 1);
    let (id, value) = first_entry(dispatch(&mut s, "XRANGE", &["s", "-", "+"]));
    assert_eq!(id, "5-5");
    assert_eq!(value, "v", "rejected XADD must not mutate the entry body");
}

#[test]
fn stale_total_order_lease_epoch_is_refused_and_stores_nothing() {
    let mut s = complete_session();
    // Keep a concrete handle so `steal` (a dev/harness-only helper) can drive the
    // fence while the session consumes the same registry via the trait object.
    let leases = Arc::new(StreamLeaseAuthority::single_node());
    s.stream_leases = leases.clone();
    let id = bulk(dispatch(&mut s, "XADD", &["s", "*", "f", "v"]));
    let (last_ms, last_seq) = parse_id(&id);
    assert_eq!(int(dispatch(&mut s, "XLEN", &["s"])), 1);

    // A `006` steal moves the epoch to 2; the persisted stream still presents 1.
    assert_eq!(leases.steal("s"), 2);
    match dispatch(&mut s, "XADD", &["s", "*", "f", "clobber"]) {
        RedisReply::Error(e) => assert!(e.contains("stale"), "{e}"),
        other => panic!("expected stale-lease refusal, got {other:?}"),
    }

    // Refused append is not an entry: length and `last_id` are unchanged.
    assert_eq!(int(dispatch(&mut s, "XLEN", &["s"])), 1);
    let (rid, value) = first_entry(dispatch(&mut s, "XRANGE", &["s", "-", "+"]));
    assert_eq!(parse_id(rid.as_bytes()), (last_ms, last_seq));
    assert_eq!(value, "v");
}

#[test]
fn no_lease_holder_refuses_write_and_does_not_fall_open() {
    let mut s = complete_session();
    s.stream_leases = Arc::new(StreamLeaseAuthority::non_granting(7));
    match dispatch(&mut s, "XADD", &["brand-new", "*", "f", "v"]) {
        RedisReply::Error(e) => assert!(e.contains("leadership lease not held"), "{e}"),
        other => panic!("expected no-holder refusal, got {other:?}"),
    }
    // The stream was never created and nothing fell open into producer order.
    assert_eq!(int(dispatch(&mut s, "XLEN", &["brand-new"])), 0);
    assert_eq!(int(dispatch(&mut s, "EXISTS", &["brand-new"])), 0);
}

#[test]
fn producer_order_creation_takes_no_lease() {
    let mut s = complete_session();
    s.stream_leases = Arc::new(StreamLeaseAuthority::non_granting(7));
    // Producer order is a creation-time option; it must not consult the lease.
    let id = bulk(dispatch(&mut s, "XADD", &["p", "ORDER", "PRODUCER", "*", "f", "v"]));
    let id = String::from_utf8(id).unwrap();
    assert!(id.contains('-'), "{id:?}");
    assert_eq!(int(dispatch(&mut s, "XLEN", &["p"])), 1);
}

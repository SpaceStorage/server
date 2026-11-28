//! `017` US3: in-process pub/sub bus behaviour (data-model §5).
//!
//! The bus is process-local and namespace-keyed. These tests drive two (or
//! three) connection sessions that share one `PubSubBus` through the public
//! `dispatch` seam, and assert delivery shape, sharded/normal interchange,
//! namespace scoping, the subscribe-mode guard, and the return to command mode.

use std::sync::{Arc, RwLock};

use bytes::Bytes;
use spacestorage_compat::DialectProfile;
use spacestorage_handler_redis::{
    ConnectionId, PubSubBus, PushFrame, RedisReply, SessionState, dispatch,
};
use spacestorage_types::ContainerCatalog;

fn session(bus: &Arc<PubSubBus>, id: u64) -> SessionState {
    let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
    let mut s = SessionState::demo(cat);
    s.profile = DialectProfile::CompleteProduct;
    s.authenticated = true;
    s.pubsub = Arc::clone(bus);
    s.connection_id = ConnectionId(id);
    s
}

fn int(r: &RedisReply) -> i64 {
    match r {
        RedisReply::Integer(i) => *i,
        other => panic!("expected integer, got {other:?}"),
    }
}

fn frames(r: RedisReply) -> Vec<RedisReply> {
    match r {
        RedisReply::Frames(f) => f,
        other => panic!("expected frames, got {other:?}"),
    }
}

#[test]
fn pubsub_sharded_and_normal_forms_share_one_bus() {
    let bus = Arc::new(PubSubBus::new());
    let mut sub = session(&bus, 1);
    let mut publisher = session(&bus, 2);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    bus.attach(&sub.namespace.clone(), ConnectionId(1), tx);

    // SSUBSCRIBE registers the sharded channel on the same bus as normal
    // subscriptions.
    assert_eq!(frames(dispatch(&mut sub, "SSUBSCRIBE", &["mix"])).len(), 1);
    // A normal PUBLISH reaches the sharded subscriber as a `message` frame.
    assert_eq!(int(&dispatch(&mut publisher, "PUBLISH", &["mix", "hello"])), 1);
    match rx.try_recv().expect("message delivered") {
        PushFrame::Message { channel, payload } => {
            assert_eq!(channel, Bytes::from_static(b"mix"));
            assert_eq!(payload, Bytes::from_static(b"hello"));
        }
        other => panic!("expected Message, got {other:?}"),
    }

    // SPUBLISH reaches a sharded subscriber too (single-node fallback).
    assert_eq!(int(&dispatch(&mut publisher, "SPUBLISH", &["mix", "again"])), 1);
    assert!(matches!(rx.try_recv(), Ok(PushFrame::Message { .. })));

    // SUNSUBSCRIBE removes it from the bus.
    assert_eq!(frames(dispatch(&mut sub, "SUNSUBSCRIBE", &["mix"])).len(), 1);
    assert_eq!(int(&dispatch(&mut publisher, "PUBLISH", &["mix", "x"])), 0);
    assert!(rx.try_recv().is_err());
}

#[test]
fn pubsub_delivery_is_namespace_scoped() {
    let bus = Arc::new(PubSubBus::new());
    let mut sub = session(&bus, 1);
    sub.namespace = "ns-a".into();
    let mut other_ns = session(&bus, 2);
    other_ns.namespace = "ns-b".into();
    let mut same_ns = session(&bus, 3);
    same_ns.namespace = "ns-a".into();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    bus.attach("ns-a", ConnectionId(1), tx);

    dispatch(&mut sub, "SUBSCRIBE", &["ch"]);

    // A publish on the same channel name in another namespace is invisible.
    assert_eq!(int(&dispatch(&mut other_ns, "PUBLISH", &["ch", "foreign"])), 0);
    assert!(rx.try_recv().is_err());

    // A publish in the subscriber's own namespace is delivered.
    assert_eq!(int(&dispatch(&mut same_ns, "PUBLISH", &["ch", "local"])), 1);
    match rx.try_recv().expect("local message delivered") {
        PushFrame::Message { payload, .. } => {
            assert_eq!(payload, Bytes::from_static(b"local"));
        }
        other => panic!("expected Message, got {other:?}"),
    }
}

#[test]
fn pubsub_pattern_delivery_includes_matched_pattern() {
    let bus = Arc::new(PubSubBus::new());
    let mut sub = session(&bus, 1);
    let mut publisher = session(&bus, 2);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    bus.attach(&sub.namespace.clone(), ConnectionId(1), tx);

    dispatch(&mut sub, "PSUBSCRIBE", &["news.*"]);
    assert_eq!(int(&dispatch(&mut publisher, "PUBLISH", &["news.tech", "hi"])), 1);
    match rx.try_recv().expect("pattern message delivered") {
        PushFrame::PMessage { pattern, channel, payload } => {
            assert_eq!(pattern, Bytes::from_static(b"news.*"));
            assert_eq!(channel, Bytes::from_static(b"news.tech"));
            assert_eq!(payload, Bytes::from_static(b"hi"));
        }
        other => panic!("expected PMessage, got {other:?}"),
    }
    // A non-matching channel is not delivered.
    assert_eq!(int(&dispatch(&mut publisher, "PUBLISH", &["sports", "x"])), 0);
    assert!(rx.try_recv().is_err());
}

#[test]
fn pubsub_subscribe_mode_blocks_other_commands_and_ping_is_array() {
    let bus = Arc::new(PubSubBus::new());
    let mut s = session(&bus, 1);
    assert_eq!(frames(dispatch(&mut s, "SUBSCRIBE", &["ch"])).len(), 1);
    assert!(s.subscribe_mode());

    match dispatch(&mut s, "SET", &["k", "v"]) {
        RedisReply::Error(e) => {
            assert!(e.contains("allowed in this context"), "{e}");
        }
        other => panic!("expected subscribe-mode refusal, got {other:?}"),
    }

    // PING in subscribe mode returns the array form.
    match dispatch(&mut s, "PING", &[]) {
        RedisReply::Frames(f) => {
            assert_eq!(f.len(), 1);
            assert_eq!(
                f[0],
                RedisReply::Array(vec![
                    RedisReply::Bulk(Some(b"pong".to_vec())),
                    RedisReply::Bulk(Some(Vec::new())),
                ])
            );
        }
        other => panic!("expected PING array frame, got {other:?}"),
    }

    // UNSUBSCRIBE-all leaves subscribe mode; commands work again.
    let conf = frames(dispatch(&mut s, "UNSUBSCRIBE", &[]));
    assert_eq!(conf.len(), 1);
    assert!(!s.subscribe_mode());
    assert!(matches!(dispatch(&mut s, "SET", &["k", "v"]), RedisReply::Ok));
    assert!(matches!(dispatch(&mut s, "GET", &["k"]), RedisReply::Bulk(Some(_))));
}

#[test]
fn pubsub_unsubscribe_with_no_channels_reports_nil() {
    let bus = Arc::new(PubSubBus::new());
    let mut s = session(&bus, 1);
    // Never subscribed: `UNSUBSCRIBE` returns a single nil-channel frame.
    let conf = frames(dispatch(&mut s, "UNSUBSCRIBE", &[]));
    assert_eq!(
        conf[0],
        RedisReply::Array(vec![
            RedisReply::Bulk(Some(b"unsubscribe".to_vec())),
            RedisReply::Bulk(None),
            RedisReply::Integer(0),
        ])
    );
}

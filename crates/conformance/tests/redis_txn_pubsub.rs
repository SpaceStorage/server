//! US3 (`017` T042): transaction and pub/sub conformance against an unmodified
//! stock Redis client.
//!
//! The live harness serves the real `RedisHandler` on an ephemeral loopback
//! port; every reply is decoded by the stock `redis` crate, so these pin the
//! RESP wire shape (queueing, `EXEC` multi-bulk, inline exec errors with no
//! rollback, `WATCH` abort, and pub/sub delivery) rather than an internal model.
#![cfg(feature = "redis-dialect")]

mod support;

use std::time::Duration;

use futures::StreamExt;
use redis::Value;

use support::redis_live::{boot_redis_complete_product, boot_redis_named};

type Conn = redis::aio::MultiplexedConnection;

// ---------------------------------------------------------------------------
// RESP plumbing
// ---------------------------------------------------------------------------

async fn q(c: &mut Conn, parts: &[&str]) -> Value {
    let mut cmd = redis::cmd(parts[0]);
    for p in &parts[1..] {
        cmd.arg(*p);
    }
    cmd.query_async::<Value>(c)
        .await
        .unwrap_or_else(|e| panic!("{parts:?} -> {e}"))
}

async fn qerr(c: &mut Conn, parts: &[&str]) -> String {
    let mut cmd = redis::cmd(parts[0]);
    for p in &parts[1..] {
        cmd.arg(*p);
    }
    match cmd.query_async::<Value>(c).await {
        Ok(v) => panic!("{parts:?}: expected error, got {v:?}"),
        Err(e) => e.to_string(),
    }
}

/// Query without `Value::extract_error`, so an error **inside** a multi-bulk
/// reply (an inline `EXEC` element) stays a `Value::ServerError` rather than
/// being flattened into a top-level command error. `Cmd::query_async` always
/// extracts nested errors; `req_packed_command` is the stock client's raw path.
async fn q_raw(c: &mut Conn, parts: &[&str]) -> Value {
    use redis::aio::ConnectionLike;
    let mut cmd = redis::cmd(parts[0]);
    for p in &parts[1..] {
        cmd.arg(*p);
    }
    c.req_packed_command(&cmd)
        .await
        .unwrap_or_else(|e| panic!("{parts:?} -> {e}"))
}

fn bulk(v: &Value) -> String {
    match v {
        Value::BulkString(b) => String::from_utf8(b.clone()).unwrap(),
        Value::SimpleString(s) => s.clone(),
        Value::Okay => "OK".into(),
        other => panic!("expected string, got {other:?}"),
    }
}

fn int(v: &Value) -> i64 {
    match v {
        Value::Int(n) => *n,
        other => panic!("expected int, got {other:?}"),
    }
}

fn arr(v: &Value) -> &[Value] {
    match v {
        Value::Array(a) => a,
        other => panic!("expected array, got {other:?}"),
    }
}

fn is_nil(v: &Value) -> bool {
    matches!(v, Value::Nil)
}

fn pubsub_client(lab: &support::redis_live::RedisLab) -> redis::Client {
    redis::Client::open(lab.url()).expect("redis url")
}

// ---------------------------------------------------------------------------
// T042 — transactions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn transactions_multi_exec_order_and_discard() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    assert_eq!(bulk(&q(&mut c, &["MULTI"]).await), "OK");
    assert_eq!(bulk(&q(&mut c, &["SET", "k1", "v1"]).await), "QUEUED");
    assert_eq!(bulk(&q(&mut c, &["SET", "k2", "v2"]).await), "QUEUED");
    assert_eq!(bulk(&q(&mut c, &["INCR", "n"]).await), "QUEUED");

    let exec = q(&mut c, &["EXEC"]).await;
    let items = arr(&exec);
    assert_eq!(items.len(), 3, "EXEC multi-bulk length: {exec:?}");
    assert_eq!(bulk(&items[0]), "OK");
    assert_eq!(bulk(&items[1]), "OK");
    assert_eq!(int(&items[2]), 1);
    assert_eq!(bulk(&q(&mut c, &["GET", "k1"]).await), "v1");
    assert_eq!(bulk(&q(&mut c, &["GET", "k2"]).await), "v2");
    assert_eq!(bulk(&q(&mut c, &["GET", "n"]).await), "1");

    // DISCARD runs nothing.
    assert_eq!(bulk(&q(&mut c, &["MULTI"]).await), "OK");
    assert_eq!(bulk(&q(&mut c, &["SET", "k3", "v3"]).await), "QUEUED");
    assert_eq!(bulk(&q(&mut c, &["DISCARD"]).await), "OK");
    assert_eq!(int(&q(&mut c, &["EXISTS", "k3"]).await), 0);
    // A fresh transaction after DISCARD works.
    assert_eq!(bulk(&q(&mut c, &["MULTI"]).await), "OK");
    assert_eq!(bulk(&q(&mut c, &["SET", "k3", "v3"]).await), "QUEUED");
    assert_eq!(arr(&q(&mut c, &["EXEC"]).await).len(), 1);
    assert_eq!(bulk(&q(&mut c, &["GET", "k3"]).await), "v3");

    lab.stop();
}

#[tokio::test]
async fn transactions_exec_error_inline_and_no_rollback() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    assert_eq!(bulk(&q(&mut c, &["SET", "str", "hello"]).await), "OK");
    assert_eq!(bulk(&q(&mut c, &["MULTI"]).await), "OK");
    assert_eq!(bulk(&q(&mut c, &["SET", "a", "1"]).await), "QUEUED");
    // Queues cleanly; fails at EXEC time with WRONGTYPE.
    assert_eq!(bulk(&q(&mut c, &["HGET", "str", "f"]).await), "QUEUED");
    assert_eq!(bulk(&q(&mut c, &["SET", "b", "2"]).await), "QUEUED");

    let exec = q_raw(&mut c, &["EXEC"]).await;
    let items = arr(&exec);
    assert_eq!(items.len(), 3, "EXEC multi-bulk: {exec:?}");
    assert_eq!(bulk(&items[0]), "OK");
    match &items[1] {
        Value::ServerError(e) => {
            let s = format!("{e:?}");
            assert!(s.contains("WRONGTYPE") || s.contains("wrong kind"), "{s}");
        }
        other => panic!("expected inline WRONGTYPE, got {other:?}"),
    }
    assert_eq!(bulk(&items[2]), "OK");

    // No rollback: the earlier and later writes remain applied.
    assert_eq!(bulk(&q(&mut c, &["GET", "a"]).await), "1");
    assert_eq!(bulk(&q(&mut c, &["GET", "b"]).await), "2");

    // A queuing-time syntax error aborts the whole block at EXEC.
    assert_eq!(bulk(&q(&mut c, &["MULTI"]).await), "OK");
    assert_eq!(bulk(&q(&mut c, &["SET", "c", "1"]).await), "QUEUED");
    let _ = qerr(&mut c, &["GET"]).await;
    let abort = qerr(&mut c, &["EXEC"]).await;
    // The stock client renders `-EXECABORT ...` as its `ExecAbortError` kind.
    assert!(
        abort.contains("ExecAbortError") || abort.contains("Transaction discarded"),
        "{abort}"
    );
    assert_eq!(int(&q(&mut c, &["EXISTS", "c"]).await), 0, "no writes on abort");

    lab.stop();
}

#[tokio::test]
async fn transactions_watch_abort_then_unwatch() {
    let lab = boot_redis_complete_product().await;
    let mut c1 = lab.connect().await;
    let mut c2 = lab.connect().await;

    assert_eq!(bulk(&q(&mut c1, &["SET", "k", "1"]).await), "OK");
    assert_eq!(bulk(&q(&mut c1, &["WATCH", "k"]).await), "OK");

    // A concurrent modification between WATCH and EXEC.
    assert_eq!(bulk(&q(&mut c2, &["SET", "k", "2"]).await), "OK");

    assert_eq!(bulk(&q(&mut c1, &["MULTI"]).await), "OK");
    assert_eq!(bulk(&q(&mut c1, &["SET", "k", "3"]).await), "QUEUED");
    let exec = q(&mut c1, &["EXEC"]).await;
    assert!(is_nil(&exec), "WATCH abort must be the null reply, got {exec:?}");
    // Nothing ran; the concurrent value stands.
    assert_eq!(bulk(&q(&mut c1, &["GET", "k"]).await), "2");

    // UNWATCH then a fresh transaction works.
    assert_eq!(bulk(&q(&mut c1, &["UNWATCH"]).await), "OK");
    assert_eq!(bulk(&q(&mut c1, &["WATCH", "k"]).await), "OK");
    assert_eq!(bulk(&q(&mut c1, &["MULTI"]).await), "OK");
    assert_eq!(bulk(&q(&mut c1, &["SET", "k", "4"]).await), "QUEUED");
    assert_eq!(arr(&q(&mut c1, &["EXEC"]).await).len(), 1);
    assert_eq!(bulk(&q(&mut c1, &["GET", "k"]).await), "4");

    lab.stop();
}

// ---------------------------------------------------------------------------
// T042 — pub/sub
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pubsub_subscribe_delivery_shape() {
    let lab = boot_redis_complete_product().await;
    let mut publisher = lab.connect().await;

    let mut sub = pubsub_client(&lab)
        .get_async_pubsub()
        .await
        .expect("pubsub connect");
    sub.subscribe("news").await.expect("subscribe");

    assert_eq!(int(&q(&mut publisher, &["PUBLISH", "news", "hello"]).await), 1);
    let msg = tokio::time::timeout(Duration::from_secs(5), sub.on_message().next())
        .await
        .expect("message timeout")
        .expect("stream ended");
    assert_eq!(msg.get_channel::<String>().unwrap(), "news");
    assert_eq!(msg.get_payload::<String>().unwrap(), "hello");
    assert!(msg.get_pattern::<String>().is_err(), "channel message has no pattern");

    lab.stop();
}

#[tokio::test]
async fn pubsub_psubscribe_pattern_shape() {
    let lab = boot_redis_complete_product().await;
    let mut publisher = lab.connect().await;

    let mut sub = pubsub_client(&lab)
        .get_async_pubsub()
        .await
        .expect("pubsub connect");
    sub.psubscribe("news.*").await.expect("psubscribe");

    assert_eq!(int(&q(&mut publisher, &["PUBLISH", "news.tech", "hi"]).await), 1);
    let msg = tokio::time::timeout(Duration::from_secs(5), sub.on_message().next())
        .await
        .expect("message timeout")
        .expect("stream ended");
    assert_eq!(msg.get_pattern::<String>().unwrap(), "news.*");
    assert_eq!(msg.get_channel::<String>().unwrap(), "news.tech");
    assert_eq!(msg.get_payload::<String>().unwrap(), "hi");

    // A non-matching channel reaches no subscriber.
    assert_eq!(int(&q(&mut publisher, &["PUBLISH", "sports", "x"]).await), 0);

    lab.stop();
}

#[tokio::test]
async fn pubsub_sharded_forms_share_bus() {
    let lab = boot_redis_complete_product().await;
    let mut ctl = lab.connect().await;

    let mut sub = pubsub_client(&lab)
        .get_async_pubsub()
        .await
        .expect("pubsub connect");
    sub.subscribe("mix").await.expect("subscribe");

    // SPUBLISH (sharded) is received by a normal SUBSCRIBE: one bus.
    assert_eq!(int(&q(&mut ctl, &["SPUBLISH", "mix", "via-shard"]).await), 1);
    let msg = tokio::time::timeout(Duration::from_secs(5), sub.on_message().next())
        .await
        .expect("message timeout")
        .expect("stream ended");
    assert_eq!(msg.get_channel::<String>().unwrap(), "mix");
    assert_eq!(msg.get_payload::<String>().unwrap(), "via-shard");

    // SSUBSCRIBE on a plain connection joins the same single-node bus.
    let mut ssub = lab.connect().await;
    let conf = q(&mut ssub, &["SSUBSCRIBE", "mix"]).await;
    assert_eq!(bulk(&arr(&conf)[0]), "ssubscribe");
    assert_eq!(bulk(&arr(&conf)[1]), "mix");
    // Redis reports this connection's own subscription count (1 here), not the
    // process-wide subscriber total (which `PUBSUB NUMSUB` below shows as 2).
    assert_eq!(int(&arr(&conf)[2]), 1, "per-connection subscription count");

    // `PUBSUB NUMSUB` sees both subscriptions on the shared bus.
    let ns = q(&mut ctl, &["PUBSUB", "NUMSUB", "mix"]).await;
    assert_eq!(bulk(&arr(&ns)[0]), "mix");
    assert_eq!(int(&arr(&ns)[1]), 2);

    // SUNSUBSCRIBE removes the sharded subscription from the same bus. No
    // publish targets "mix" while `ssub` is subscribed, so its next reply is
    // the confirmation (no out-of-band message queued ahead of it).
    let conf = q(&mut ssub, &["SUNSUBSCRIBE", "mix"]).await;
    assert_eq!(bulk(&arr(&conf)[0]), "sunsubscribe");
    assert_eq!(bulk(&arr(&conf)[1]), "mix");
    let ns = q(&mut ctl, &["PUBSUB", "NUMSUB", "mix"]).await;
    assert_eq!(int(&arr(&ns)[1]), 1);

    lab.stop();
}

#[tokio::test]
async fn pubsub_unsubscribe_all_returns_to_command_mode() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    let conf = q(&mut c, &["SUBSCRIBE", "solo"]).await;
    assert_eq!(bulk(&arr(&conf)[0]), "subscribe");
    assert_eq!(bulk(&arr(&conf)[1]), "solo");
    assert_eq!(int(&arr(&conf)[2]), 1);

    // In subscribe mode only the documented verbs are allowed.
    let e = qerr(&mut c, &["SET", "k", "v"]).await;
    assert!(e.contains("allowed in this context"), "{e}");

    // UNSUBSCRIBE-all (no arguments) leaves subscribe mode.
    let conf = q(&mut c, &["UNSUBSCRIBE"]).await;
    let a = arr(&conf);
    assert_eq!(bulk(&a[0]), "unsubscribe");
    assert_eq!(bulk(&a[1]), "solo");
    assert_eq!(int(&a[2]), 0);

    // The connection is usable for regular commands again.
    assert_eq!(bulk(&q(&mut c, &["SET", "k", "v"]).await), "OK");
    assert_eq!(bulk(&q(&mut c, &["GET", "k"]).await), "v");

    lab.stop();
}

#[tokio::test]
async fn pubsub_namespace_isolation() {
    let lab_a = boot_redis_named("ns-a").await;
    let lab_b = boot_redis_named("ns-b").await;
    let mut pub_a = lab_a.connect().await;
    let mut pub_b = lab_b.connect().await;

    let mut sub_a = pubsub_client(&lab_a)
        .get_async_pubsub()
        .await
        .expect("pubsub connect");
    sub_a.subscribe("ch").await.expect("subscribe");

    // Same channel name in another namespace, but nothing is delivered.
    assert_eq!(int(&q(&mut pub_b, &["PUBLISH", "ch", "from-b"]).await), 0);
    // The subscriber's own namespace delivers.
    assert_eq!(int(&q(&mut pub_a, &["PUBLISH", "ch", "from-a"]).await), 1);
    let msg = tokio::time::timeout(Duration::from_secs(5), sub_a.on_message().next())
        .await
        .expect("message timeout")
        .expect("stream ended");
    assert_eq!(msg.get_payload::<String>().unwrap(), "from-a");

    lab_a.stop();
    lab_b.stop();
}

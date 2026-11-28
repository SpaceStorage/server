//! US2 (`017` T031–T032): stream + consumer-group conformance against an
//! unmodified stock Redis client, plus a same-data-dir restart durability case.
//!
//! The live harness serves the real `RedisHandler` on an ephemeral loopback
//! port over the shared catalog; every reply below is decoded by the stock
//! `redis` crate, so these pin the RESP wire shape, not an internal model.
#![cfg(feature = "redis-dialect")]

mod support;

use std::collections::BTreeMap;
use std::time::Duration;

use redis::Value;

use support::redis_live::boot_redis_complete_product;

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

/// `[[id, [field, value, …]], …]` → owned `(id, [(field, value)])` list.
fn entries(v: &Value) -> Vec<(String, Vec<(String, String)>)> {
    arr(v)
        .iter()
        .map(|e| {
            let ea = arr(e);
            let id = bulk(&ea[0]);
            let fs = arr(&ea[1]);
            let mut fields = Vec::new();
            let mut i = 0;
            while i + 1 < fs.len() {
                fields.push((bulk(&fs[i]), bulk(&fs[i + 1])));
                i += 2;
            }
            (id, fields)
        })
        .collect()
}

/// First stream of an `XREAD`/`XREADGROUP` reply → `(key, entries)`.
fn xread_first(v: &Value) -> (String, Vec<(String, Vec<(String, String)>)>) {
    let outer = arr(v);
    let inner = arr(&outer[0]);
    (bulk(&inner[0]), entries(&inner[1]))
}

/// Flat `[k, v, k, v, …]` array → map (XINFO field/value pairs).
fn flat(v: &Value) -> BTreeMap<String, Value> {
    let a = arr(v);
    let mut m = BTreeMap::new();
    let mut i = 0;
    while i + 1 < a.len() {
        m.insert(bulk(&a[i]), a[i + 1].clone());
        i += 2;
    }
    m
}

// ---------------------------------------------------------------------------
// T031 — XADD / XLEN / XRANGE / XREVRANGE
// ---------------------------------------------------------------------------

#[tokio::test]
async fn streams_xadd_xlen_xrange_xrevrange() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    let id1 = bulk(&q(&mut c, &["XADD", "s", "*", "f1", "v1"]).await);
    let id2 = bulk(&q(&mut c, &["XADD", "s", "*", "f2", "v2"]).await);
    assert_eq!(int(&q(&mut c, &["XLEN", "s"]).await), 2);

    let all = q(&mut c, &["XRANGE", "s", "-", "+"]).await;
    let es = entries(&all);
    assert_eq!(es.len(), 2);
    assert_eq!(es[0].0, id1);
    assert_eq!(es[0].1, vec![("f1".to_string(), "v1".to_string())]);
    assert_eq!(es[1].0, id2);

    let counted = q(&mut c, &["XRANGE", "s", "-", "+", "COUNT", "1"]).await;
    assert_eq!(entries(&counted).len(), 1);

    let single = q(&mut c, &["XRANGE", "s", id1.as_str(), id1.as_str()]).await;
    let se = entries(&single);
    assert_eq!(se.len(), 1);
    assert_eq!(se[0].0, id1);

    let rev = q(&mut c, &["XREVRANGE", "s", "+", "-"]).await;
    let re = entries(&rev);
    assert_eq!(re[0].0, id2);
    assert_eq!(re[1].0, id1);

    lab.stop();
}

#[tokio::test]
async fn streams_xadd_explicit_ids_and_nomkstream() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    assert_eq!(bulk(&q(&mut c, &["XADD", "e", "1-1", "f", "v"]).await), "1-1");
    let err = qerr(&mut c, &["XADD", "e", "1-1", "f", "x"]).await;
    assert!(err.contains("equal or smaller"), "{err}");
    assert_eq!(bulk(&q(&mut c, &["XADD", "e", "1-2", "f", "w"]).await), "1-2");
    assert_eq!(int(&q(&mut c, &["XLEN", "e"]).await), 2);

    // NOMKSTREAM on a missing key: nil, and the stream is not created.
    let r = q(&mut c, &["XADD", "nomk", "NOMKSTREAM", "*", "f", "v"]).await;
    assert!(is_nil(&r), "{r:?}");
    assert_eq!(int(&q(&mut c, &["EXISTS", "nomk"]).await), 0);

    lab.stop();
}

// ---------------------------------------------------------------------------
// T031 — XREAD (sync, COUNT, BLOCK)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn streams_xread_sync_and_count() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    let id1 = bulk(&q(&mut c, &["XADD", "s", "1-1", "a", "1"]).await);
    let id2 = bulk(&q(&mut c, &["XADD", "s", "2-2", "b", "2"]).await);

    let all = q(&mut c, &["XREAD", "STREAMS", "s", "0"]).await;
    let (key, es) = xread_first(&all);
    assert_eq!(key, "s");
    assert_eq!(es.len(), 2);
    assert_eq!(es[0].0, id1);
    assert_eq!(es[1].0, id2);

    let after = q(&mut c, &["XREAD", "STREAMS", "s", id1.as_str()]).await;
    let (_, es) = xread_first(&after);
    assert_eq!(es.len(), 1);
    assert_eq!(es[0].0, id2);

    let counted = q(&mut c, &["XREAD", "COUNT", "1", "STREAMS", "s", "0"]).await;
    assert_eq!(xread_first(&counted).1.len(), 1);

    // `$` on an existing stream = its current last id → no new entry → nil.
    let tail = q(&mut c, &["XREAD", "STREAMS", "s", "$"]).await;
    assert!(is_nil(&tail), "{tail:?}");
    // Missing key → nil.
    let missing = q(&mut c, &["XREAD", "STREAMS", "missing", "0"]).await;
    assert!(is_nil(&missing), "{missing:?}");

    lab.stop();
}

#[tokio::test]
async fn streams_xread_block_timeout_and_wake() {
    let lab = boot_redis_complete_product().await;
    let mut c1 = lab.connect().await;

    // Timeout path: empty stream, 120 ms BLOCK → nil.
    let to = q(&mut c1, &["XREAD", "BLOCK", "120", "STREAMS", "blk", "$"]).await;
    assert!(is_nil(&to), "{to:?}");

    // Wake path: a second connection appends after the block is established.
    let mut c2 = lab.connect().await;
    let pusher = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        let _: Value = q(&mut c2, &["XADD", "blk", "*", "k", "v"]).await;
    });
    let woken = q(&mut c1, &["XREAD", "BLOCK", "3000", "STREAMS", "blk", "$"]).await;
    pusher.await.unwrap();
    let (key, es) = xread_first(&woken);
    assert_eq!(key, "blk");
    assert_eq!(es.len(), 1);
    assert_eq!(es[0].1, vec![("k".to_string(), "v".to_string())]);

    lab.stop();
}

// ---------------------------------------------------------------------------
// T031 — XGROUP family
// ---------------------------------------------------------------------------

#[tokio::test]
async fn streams_xgroup_family() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    let _ = q(&mut c, &["XADD", "s", "1-1", "f", "v"]).await;
    assert_eq!(bulk(&q(&mut c, &["XGROUP", "CREATE", "s", "g1", "0"]).await), "OK");

    let dup = qerr(&mut c, &["XGROUP", "CREATE", "s", "g1", "0"]).await;
    assert!(dup.contains("BUSYGROUP"), "{dup}");

    let missing = qerr(&mut c, &["XGROUP", "CREATE", "nokey", "g", "0"]).await;
    assert!(missing.contains("requires the key to exist"), "{missing}");

    assert_eq!(
        bulk(&q(&mut c, &["XGROUP", "CREATE", "mk", "g", "$", "MKSTREAM"]).await),
        "OK"
    );
    assert_eq!(int(&q(&mut c, &["XLEN", "mk"]).await), 0);

    assert_eq!(int(&q(&mut c, &["XGROUP", "CREATECONSUMER", "s", "g1", "c1"]).await), 1);
    assert_eq!(int(&q(&mut c, &["XGROUP", "CREATECONSUMER", "s", "g1", "c1"]).await), 0);

    let help = q(&mut c, &["XGROUP", "HELP"]).await;
    assert!(arr(&help).len() >= 3, "XGROUP HELP too short");

    assert_eq!(bulk(&q(&mut c, &["XGROUP", "SETID", "s", "g1", "0"]).await), "OK");
    assert_eq!(int(&q(&mut c, &["XGROUP", "DELCONSUMER", "s", "g1", "c1"]).await), 0);
    assert_eq!(int(&q(&mut c, &["XGROUP", "DESTROY", "s", "g1"]).await), 1);
    assert_eq!(int(&q(&mut c, &["XGROUP", "DESTROY", "s", "g1"]).await), 0);

    lab.stop();
}

// ---------------------------------------------------------------------------
// T031 — XREADGROUP / XPENDING / XACK / XCLAIM
// ---------------------------------------------------------------------------

#[tokio::test]
async fn streams_xreadgroup_pending_ack_claim() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    let id1 = bulk(&q(&mut c, &["XADD", "s", "1-1", "f", "v1"]).await);
    let id2 = bulk(&q(&mut c, &["XADD", "s", "2-2", "f", "v2"]).await);
    let id3 = bulk(&q(&mut c, &["XADD", "s", "3-3", "f", "v3"]).await);
    assert_eq!(bulk(&q(&mut c, &["XGROUP", "CREATE", "s", "g", "0"]).await), "OK");

    // At-most-one-consumer delivery: c1 takes two, c2 takes the rest.
    let r = q(
        &mut c,
        &["XREADGROUP", "GROUP", "g", "c1", "COUNT", "2", "STREAMS", "s", ">"],
    )
    .await;
    let (key, es) = xread_first(&r);
    assert_eq!(key, "s");
    assert_eq!(es.len(), 2);
    assert_eq!(es[0].0, id1);
    assert_eq!(es[1].0, id2);

    let r = q(&mut c, &["XREADGROUP", "GROUP", "g", "c2", "STREAMS", "s", ">"]).await;
    let (_, es) = xread_first(&r);
    assert_eq!(es.len(), 1);
    assert_eq!(es[0].0, id3);

    // Summary form: [count, min, max, [[consumer, count]…]].
    let p = q(&mut c, &["XPENDING", "s", "g"]).await;
    let pa = arr(&p);
    assert_eq!(int(&pa[0]), 3);
    assert_eq!(bulk(&pa[1]), id1);
    assert_eq!(bulk(&pa[2]), id3);
    assert_eq!(arr(&pa[3]).len(), 2);

    // Extended form: [start end count] and per-consumer filter.
    let ext = q(&mut c, &["XPENDING", "s", "g", "-", "+", "10"]).await;
    assert_eq!(arr(&ext).len(), 3);
    let ext_c1 = q(&mut c, &["XPENDING", "s", "g", "-", "+", "10", "c1"]).await;
    assert_eq!(arr(&ext_c1).len(), 2);

    // XACK removes pending once.
    assert_eq!(int(&q(&mut c, &["XACK", "s", "g", id1.as_str()]).await), 1);
    assert_eq!(int(&q(&mut c, &["XACK", "s", "g", id1.as_str()]).await), 0);
    let p2 = q(&mut c, &["XPENDING", "s", "g"]).await;
    assert_eq!(int(&arr(&p2)[0]), 2);

    // XCLAIM JUSTID reassigns ownership of id2 to c2.
    let claim = q(&mut c, &["XCLAIM", "s", "g", "c2", "0", id2.as_str(), "JUSTID"]).await;
    let ca = arr(&claim);
    assert_eq!(ca.len(), 1);
    assert_eq!(bulk(&ca[0]), id2);
    let c2p = q(&mut c, &["XPENDING", "s", "g", "-", "+", "10", "c2"]).await;
    assert_eq!(arr(&c2p).len(), 2);

    // XCLAIM without JUSTID returns the entry body.
    let claim2 = q(&mut c, &["XCLAIM", "s", "g", "c1", "0", id3.as_str()]).await;
    let c2a = arr(&claim2);
    assert_eq!(c2a.len(), 1);
    assert_eq!(bulk(&arr(&c2a[0])[0]), id3);

    // `XREADGROUP … 0` replays c1's own pending entry.
    let replay = q(&mut c, &["XREADGROUP", "GROUP", "g", "c1", "STREAMS", "s", "0"]).await;
    let (_, es) = xread_first(&replay);
    assert_eq!(es.len(), 1);
    assert_eq!(es[0].0, id3);

    lab.stop();
}

#[tokio::test]
async fn streams_xreadgroup_block_timeout() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    let _ = q(&mut c, &["XADD", "s", "*", "f", "v"]).await;
    assert_eq!(bulk(&q(&mut c, &["XGROUP", "CREATE", "s", "g", "0"]).await), "OK");
    // Drain the stream so the group has no new entries.
    let _ = q(&mut c, &["XREADGROUP", "GROUP", "g", "c", "STREAMS", "s", ">"]).await;
    // No new entries → BLOCK 120 ms → nil.
    let to = q(
        &mut c,
        &["XREADGROUP", "GROUP", "g", "c", "BLOCK", "120", "STREAMS", "s", ">"],
    )
    .await;
    assert!(is_nil(&to), "{to:?}");

    lab.stop();
}

// ---------------------------------------------------------------------------
// T031 — XDEL / XTRIM
// ---------------------------------------------------------------------------

#[tokio::test]
async fn streams_xdel_xtrim() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    let _ = q(&mut c, &["XADD", "s", "1-1", "f", "1"]).await;
    let i2 = bulk(&q(&mut c, &["XADD", "s", "2-2", "f", "2"]).await);
    let i3 = bulk(&q(&mut c, &["XADD", "s", "3-3", "f", "3"]).await);
    let i4 = bulk(&q(&mut c, &["XADD", "s", "4-4", "f", "4"]).await);
    assert_eq!(int(&q(&mut c, &["XLEN", "s"]).await), 4);

    assert_eq!(int(&q(&mut c, &["XDEL", "s", i2.as_str()]).await), 1);
    assert_eq!(int(&q(&mut c, &["XDEL", "s", i2.as_str()]).await), 0);
    assert_eq!(int(&q(&mut c, &["XLEN", "s"]).await), 3);

    assert_eq!(int(&q(&mut c, &["XTRIM", "s", "MAXLEN", "2"]).await), 1);
    assert_eq!(int(&q(&mut c, &["XLEN", "s"]).await), 2);
    let es = entries(&q(&mut c, &["XRANGE", "s", "-", "+"]).await);
    assert_eq!(es[0].0, i3);
    assert_eq!(es[1].0, i4);

    assert_eq!(int(&q(&mut c, &["XTRIM", "s", "MINID", i4.as_str()]).await), 1);
    let es = entries(&q(&mut c, &["XRANGE", "s", "-", "+"]).await);
    assert_eq!(es.len(), 1);
    assert_eq!(es[0].0, i4);

    // Approximate marker is accepted (`~`).
    assert_eq!(int(&q(&mut c, &["XTRIM", "s", "MAXLEN", "~", "0"]).await), 1);
    assert_eq!(int(&q(&mut c, &["XLEN", "s"]).await), 0);

    lab.stop();
}

#[tokio::test]
async fn streams_trim_drops_pending() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    for i in 1..=3 {
        let _ = q(&mut c, &["XADD", "u", "*", "f", &i.to_string()]).await;
    }
    assert_eq!(bulk(&q(&mut c, &["XGROUP", "CREATE", "u", "g", "0"]).await), "OK");
    let _ = q(&mut c, &["XREADGROUP", "GROUP", "g", "c", "STREAMS", "u", ">"]).await;
    let p = q(&mut c, &["XPENDING", "u", "g"]).await;
    assert_eq!(int(&arr(&p)[0]), 3);

    // Trimming drops the two oldest entries *and* their PEL records.
    assert_eq!(int(&q(&mut c, &["XTRIM", "u", "MAXLEN", "1"]).await), 2);
    let p = q(&mut c, &["XPENDING", "u", "g"]).await;
    assert_eq!(int(&arr(&p)[0]), 1);
    assert_eq!(int(&q(&mut c, &["XLEN", "u"]).await), 1);

    lab.stop();
}

// ---------------------------------------------------------------------------
// T031 — XINFO
// ---------------------------------------------------------------------------

#[tokio::test]
async fn streams_xinfo() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    let id1 = bulk(&q(&mut c, &["XADD", "s", "*", "f", "v"]).await);
    assert_eq!(bulk(&q(&mut c, &["XGROUP", "CREATE", "s", "g", "0"]).await), "OK");
    let _ = q(&mut c, &["XREADGROUP", "GROUP", "g", "c1", "STREAMS", "s", ">"]).await;

    let stream = q(&mut c, &["XINFO", "STREAM", "s"]).await;
    let info = flat(&stream);
    assert_eq!(int(&info["length"]), 1);
    assert_eq!(int(&info["groups"]), 1);
    assert_eq!(bulk(&info["last-generated-id"]), id1);

    let groups = q(&mut c, &["XINFO", "GROUPS", "s"]).await;
    let ga = arr(&groups);
    assert_eq!(ga.len(), 1);
    let g = flat(&ga[0]);
    assert_eq!(bulk(&g["name"]), "g");
    assert_eq!(int(&g["pending"]), 1);

    let consumers = q(&mut c, &["XINFO", "CONSUMERS", "s", "g"]).await;
    let ca = arr(&consumers);
    assert_eq!(ca.len(), 1);
    let con = flat(&ca[0]);
    assert_eq!(bulk(&con["name"]), "c1");
    assert_eq!(int(&con["pending"]), 1);

    lab.stop();
}

// ---------------------------------------------------------------------------
// T031 / T041 — not-supported + type safety
// ---------------------------------------------------------------------------

#[tokio::test]
async fn streams_xautoclaim_rejected() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    let err = qerr(&mut c, &["XAUTOCLAIM", "s", "g", "c", "0", "0"]).await;
    assert!(err.contains("unknown command") && err.contains("XAUTOCLAIM"), "{err}");

    lab.stop();
}

#[tokio::test]
async fn streams_wrongtype_on_string() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    let _ = q(&mut c, &["SET", "str", "v"]).await;
    let err = qerr(&mut c, &["XADD", "str", "*", "f", "v"]).await;
    assert!(err.starts_with("WRONGTYPE"), "{err}");
    let err = qerr(&mut c, &["XREAD", "STREAMS", "str", "0"]).await;
    assert!(err.starts_with("WRONGTYPE"), "{err}");

    lab.stop();
}

// ---------------------------------------------------------------------------
// T032 — restart durability (persistent stream)
// ---------------------------------------------------------------------------

/// A persistent stream restores content, `last_id`, group `last_delivered`, and
/// pending state across a reboot of the same data dir (FR-016). Memory mode is
/// explicitly *not* promised and is not asserted here.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn streams_restart_restores_content_offsets_pending() {
    use std::sync::Arc;

    use spacestorage_compat::DialectProfile;
    use spacestorage_conformance::boot_one_node;
    use spacestorage_handler_redis::{RedisReply, SessionState, dispatch};
    use tempfile::tempdir;

    fn complete(session: &mut SessionState) {
        session.profile = DialectProfile::CompleteProduct;
        assert!(matches!(
            dispatch(session, "AUTH", &["demo", "demo"]),
            RedisReply::Ok
        ));
    }
    fn bulk_of(r: RedisReply) -> Vec<u8> {
        match r {
            RedisReply::Bulk(Some(b)) => b,
            other => panic!("expected bulk, got {other:?}"),
        }
    }
    fn int_of(r: RedisReply) -> i64 {
        match r {
            RedisReply::Integer(i) => i,
            other => panic!("expected int, got {other:?}"),
        }
    }

    let dir = tempdir().unwrap();
    let work = dir.path().to_path_buf();

    let (id1, id2) = {
        let lab = boot_one_node(&work).await;
        assert!(
            lab.node.storage.read().await.is_some(),
            "storage engine must be open for durable streams"
        );
        let mut s = SessionState::demo(Arc::clone(&lab.node.catalog));
        complete(&mut s);
        let id1 = bulk_of(dispatch(&mut s, "XADD", &["s", "1-1", "f", "v1"]));
        let id2 = bulk_of(dispatch(&mut s, "XADD", &["s", "2-2", "f", "v2"]));
        assert!(matches!(
            dispatch(&mut s, "XGROUP", &["CREATE", "s", "g", "0"]),
            RedisReply::Ok
        ));
        // Deliver both, then ack the first: one pending entry survives.
        match dispatch(
            &mut s,
            "XREADGROUP",
            &["GROUP", "g", "c1", "STREAMS", "s", ">"],
        ) {
            RedisReply::Array(streams) => assert_eq!(streams.len(), 1),
            other => panic!("XREADGROUP: {other:?}"),
        }
        assert_eq!(int_of(dispatch(&mut s, "XACK", &["s", "g", "1-1"])), 1);
        lab.shutdown().await;
        (id1, id2)
    };

    // Reboot the same data dir: content + group positions rehydrate from WAL.
    let lab = boot_one_node(&work).await;
    let mut s = SessionState::demo(Arc::clone(&lab.node.catalog));
    complete(&mut s);

    assert_eq!(int_of(dispatch(&mut s, "XLEN", &["s"])), 2, "content lost");
    match dispatch(&mut s, "XRANGE", &["s", "-", "+"]) {
        RedisReply::Array(items) => {
            assert_eq!(items.len(), 2);
            match &items[0] {
                RedisReply::Array(first) => {
                    assert_eq!(first[0], RedisReply::Bulk(Some(id1.clone())), "first entry id");
                }
                other => panic!("first entry not array: {other:?}"),
            }
        }
        other => panic!("XRANGE after reboot: {other:?}"),
    }

    // Group offset (last-delivered-id) and pending state survive.
    match dispatch(&mut s, "XPENDING", &["s", "g"]) {
        RedisReply::Array(items) => {
            assert_eq!(int_of(items[0].clone()), 1, "pending count after reboot");
        }
        other => panic!("XPENDING after reboot: {other:?}"),
    }
    match dispatch(&mut s, "XINFO", &["GROUPS", "s"]) {
        RedisReply::Array(items) => {
            assert_eq!(items.len(), 1);
            // last-delivered-id must survive (2-2) — the group did not reset.
            let group = items[0].clone();
            let flat: Vec<RedisReply> = match group {
                RedisReply::Array(a) => a,
                other => panic!("group not array: {other:?}"),
            };
            let find = |name: &str| -> Option<RedisReply> {
                flat.iter()
                    .enumerate()
                    .find(|(_, v)| matches!(v, RedisReply::Bulk(Some(b)) if b == name.as_bytes()))
                    .and_then(|(i, _)| flat.get(i + 1).cloned())
            };
            match find("last-delivered-id") {
                Some(RedisReply::Bulk(Some(v))) => {
                    assert_eq!(v, id2, "group last-delivered-id after reboot");
                }
                other => panic!("last-delivered-id: {other:?}"),
            }
        }
        other => panic!("XINFO GROUPS after reboot: {other:?}"),
    }

    lab.shutdown().await;
}

/// FR-016: memory-mode durability is *not* promised. A memory-mode stream's
/// content does not reappear after a reboot of the same data dir.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn streams_memory_mode_not_promised() {
    use std::sync::Arc;

    use spacestorage_compat::DialectProfile;
    use spacestorage_conformance::boot_one_node;
    use spacestorage_handler_redis::{RedisReply, SessionState, dispatch};
    use spacestorage_types::{L3Model, StorageModeChoice};
    use tempfile::tempdir;

    fn complete(session: &mut SessionState) {
        session.profile = DialectProfile::CompleteProduct;
        assert!(matches!(
            dispatch(session, "AUTH", &["demo", "demo"]),
            RedisReply::Ok
        ));
    }

    let dir = tempdir().unwrap();
    let work = dir.path().to_path_buf();

    {
        let lab = boot_one_node(&work).await;
        // A memory-mode key-space container: writes are not WAL-backed.
        lab.node
            .catalog
            .write()
            .unwrap()
            .create(
                "demo",
                "mem",
                L3Model::KvStore,
                false,
                None,
                StorageModeChoice::Memory,
            )
            .unwrap();
        let mut s = SessionState::demo(Arc::clone(&lab.node.catalog));
        s.container = "mem".into();
        complete(&mut s);
        assert!(matches!(
            dispatch(&mut s, "XADD", &["s", "*", "f", "v"]),
            RedisReply::Bulk(Some(_))
        ));
        assert!(matches!(dispatch(&mut s, "XLEN", &["s"]), RedisReply::Integer(1)));
        lab.shutdown().await;
    }

    let lab = boot_one_node(&work).await;
    let mut s = SessionState::demo(Arc::clone(&lab.node.catalog));
    s.container = "mem".into();
    complete(&mut s);
    match dispatch(&mut s, "XLEN", &["s"]) {
        RedisReply::Integer(0) => {}
        RedisReply::Integer(n) => {
            panic!("memory-mode stream unexpectedly survived reboot with {n} entries")
        }
        other => panic!("XLEN after reboot: {other:?}"),
    }
    lab.shutdown().await;
}
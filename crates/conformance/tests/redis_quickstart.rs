//! `017` T068 — execuable mirror of
//! `specs/017-redis-dialect-coverage/quickstart.md`.
//!
//! The quickstart is written for `redis-cli` against a running node. This suite
//! runs the same command sequences in-process against the real `RedisHandler` on
//! an ephemeral loopback port, decoded by an unmodified stock `redis` client, so
//! Steps 1–11 are a CI gate rather than a manual procedure. Step 11's
//! not-supported checks are mirrored here, and the first-binary negative check
//! (`quickstart.md` "Next Steps" #2) confirms no complete-product row leaks into
//! the `016` K/V profile.
#![cfg(feature = "redis-dialect")]

mod support;

use std::time::Duration;

use futures::StreamExt;
use redis::Value;
use spacestorage_compat::DialectProfile;

use support::redis_live::{boot_redis_complete_product, boot_redis_with_profile, RedisLab};

type Conn = redis::aio::MultiplexedConnection;

// ---------------------------------------------------------------------------
// RESP plumbing
// ---------------------------------------------------------------------------

/// Run `parts` and decode the first reply.
async fn q<T: redis::FromRedisValue>(c: &mut Conn, parts: &[&str]) -> T {
    let mut cmd = redis::cmd(parts[0]);
    for p in &parts[1..] {
        cmd.arg(*p);
    }
    cmd.query_async(c)
        .await
        .unwrap_or_else(|e| panic!("{parts:?} -> {e}"))
}

/// Run `parts`, mapping any server error to its rendered text.
async fn run(c: &mut Conn, parts: &[&str]) -> Result<Value, String> {
    let mut cmd = redis::cmd(parts[0]);
    for p in &parts[1..] {
        cmd.arg(*p);
    }
    match tokio::time::timeout(Duration::from_secs(5), cmd.query_async::<Value>(c)).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => panic!("{parts:?} hung past 5s"),
    }
}

async fn qerr(c: &mut Conn, parts: &[&str]) -> String {
    match run(c, parts).await {
        Ok(v) => panic!("{parts:?}: expected error, got {v:?}"),
        Err(e) => e,
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

fn pubsub_client(lab: &RedisLab) -> redis::Client {
    redis::Client::open(lab.url()).expect("redis url")
}

// ---------------------------------------------------------------------------
// Step 1: Connection and Auth
// ---------------------------------------------------------------------------

#[tokio::test]
async fn quickstart_step1_connection_and_auth() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await; // URL carries demo:demo and AUTHs.

    assert_eq!(q::<String>(&mut c, &["AUTH", "demo", "demo"]).await, "OK");
    assert_eq!(q::<String>(&mut c, &["PING"]).await, "PONG");
    assert_eq!(q::<String>(&mut c, &["PING", "hello"]).await, "hello");

    // SELECT 0 is a no-op; SELECT n>0 is the documented `002` error.
    assert_eq!(q::<String>(&mut c, &["SELECT", "0"]).await, "OK");
    let e = qerr(&mut c, &["SELECT", "1"]).await;
    assert!(e.contains("SELECT is not supported"), "SELECT 1 -> {e}");

    lab.stop();
}

// ---------------------------------------------------------------------------
// Step 2: Strings and Basic Operations
// ---------------------------------------------------------------------------

#[tokio::test]
async fn quickstart_step2_strings() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    assert_eq!(q::<String>(&mut c, &["SET", "mykey", "hello world"]).await, "OK");
    assert_eq!(q::<String>(&mut c, &["GET", "mykey"]).await, "hello world");
    assert_eq!(q::<i64>(&mut c, &["EXISTS", "mykey"]).await, 1);
    assert_eq!(q::<String>(&mut c, &["TYPE", "mykey"]).await, "string");
    assert_eq!(q::<i64>(&mut c, &["TTL", "mykey"]).await, -1);
    assert_eq!(q::<i64>(&mut c, &["EXPIRE", "mykey", "60"]).await, 1);
    let ttl = q::<i64>(&mut c, &["TTL", "mykey"]).await;
    assert!(ttl > 0 && ttl <= 60, "TTL after EXPIRE 60: {ttl}");
    assert_eq!(q::<i64>(&mut c, &["DEL", "mykey"]).await, 1);
    assert_eq!(q::<Option<String>>(&mut c, &["GET", "mykey"]).await, None);

    assert_eq!(q::<String>(&mut c, &["MSET", "a", "1", "b", "2", "c", "3"]).await, "OK");
    let m: Vec<Option<String>> = q(&mut c, &["MGET", "a", "b", "c", "nonexistent"]).await;
    assert_eq!(
        m,
        vec![Some("1".into()), Some("2".into()), Some("3".into()), None]
    );

    lab.stop();
}

// ---------------------------------------------------------------------------
// Step 3: Collection Types
// ---------------------------------------------------------------------------

#[tokio::test]
async fn quickstart_step3_collections() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    // Hash.
    assert_eq!(q::<i64>(&mut c, &["HSET", "user:1000", "name", "Alice", "age", "30"]).await, 2);
    assert_eq!(q::<Option<String>>(&mut c, &["HGET", "user:1000", "name"]).await.as_deref(), Some("Alice"));
    // quickstart.md line 109 lists HGETALL fields in insertion order. HGETALL
    // field order is unspecified by Redis, and this handler emits fields in
    // lexicographic order ("age" before "name"), so assert the field/value
    // content as a map rather than the doc's illustrative ordering. Recorded
    // follow-up: quickstart.md Step 3 HGETALL listing is not an order guarantee.
    let all: Vec<String> = q(&mut c, &["HGETALL", "user:1000"]).await;
    let mut pairs: Vec<(String, String)> = all
        .chunks_exact(2)
        .map(|kv| (kv[0].clone(), kv[1].clone()))
        .collect();
    pairs.sort();
    assert_eq!(
        pairs,
        vec![
            ("age".to_string(), "30".to_string()),
            ("name".to_string(), "Alice".to_string()),
        ]
    );
    assert_eq!(q::<i64>(&mut c, &["HLEN", "user:1000"]).await, 2);
    assert_eq!(q::<String>(&mut c, &["TYPE", "user:1000"]).await, "hash");

    // WRONGTYPE.
    assert_eq!(q::<String>(&mut c, &["SET", "mystring", "hi"]).await, "OK");
    let e = qerr(&mut c, &["HGET", "mystring", "field"]).await;
    assert!(e.contains("WRONGTYPE"), "HGET on a string -> {e}");

    // List.
    assert_eq!(q::<i64>(&mut c, &["LPUSH", "mylist", "first", "second"]).await, 2);
    assert_eq!(q::<i64>(&mut c, &["RPUSH", "mylist", "third"]).await, 3);
    let lr: Vec<String> = q(&mut c, &["LRANGE", "mylist", "0", "-1"]).await;
    assert_eq!(lr, vec!["second", "first", "third"]);
    assert_eq!(q::<Option<String>>(&mut c, &["LPOP", "mylist"]).await.as_deref(), Some("second"));
    assert_eq!(q::<i64>(&mut c, &["LLEN", "mylist"]).await, 2);

    // Set (SMEMBERS is unordered; assert the set contents).
    assert_eq!(q::<i64>(&mut c, &["SADD", "myset", "a", "b", "c"]).await, 3);
    let mut members: Vec<String> = q(&mut c, &["SMEMBERS", "myset"]).await;
    members.sort();
    assert_eq!(members, vec!["a", "b", "c"]);
    assert_eq!(q::<i64>(&mut c, &["SISMEMBER", "myset", "b"]).await, 1);
    assert_eq!(q::<i64>(&mut c, &["SISMEMBER", "myset", "z"]).await, 0);

    // Sorted set.
    assert_eq!(q::<i64>(&mut c, &["ZADD", "scores", "100", "alice", "200", "bob", "150", "carol"]).await, 3);
    assert_eq!(q::<i64>(&mut c, &["ZRANK", "scores", "bob"]).await, 2);
    let zr: Vec<String> = q(&mut c, &["ZRANGE", "scores", "0", "-1", "WITHSCORES"]).await;
    assert_eq!(zr, vec!["alice", "100", "carol", "150", "bob", "200"]);
    assert_eq!(q::<Option<String>>(&mut c, &["ZSCORE", "scores", "carol"]).await.as_deref(), Some("150"));

    lab.stop();
}

// ---------------------------------------------------------------------------
// Step 4: Streams and Consumer Groups
// ---------------------------------------------------------------------------

#[tokio::test]
async fn quickstart_step4_streams_and_groups() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    let id1: String = q(&mut c, &["XADD", "mystream", "*", "sensor-id", "1234", "temperature", "19.5"]).await;
    assert!(!id1.is_empty(), "XADD returns an entry id");
    let id2: String = q(&mut c, &["XADD", "mystream", "*", "sensor-id", "1235", "temperature", "20.1"]).await;
    assert!(!id2.is_empty());
    assert_eq!(q::<i64>(&mut c, &["XLEN", "mystream"]).await, 2);

    let range = q::<Value>(&mut c, &["XRANGE", "mystream", "-", "+"]).await;
    assert_eq!(arr(&range).len(), 2, "two entries: {range:?}");

    // quickstart.md line 199 creates the group at `$` and then (line 202) expects
    // `XREADGROUP ... >` to yield "One pending entry". Those cannot both hold: `$`
    // pins the group to the stream's current last id, so `>` (never-delivered
    // entries) has nothing to return — stock Redis returns nil too. To
    // demonstrate the documented outcome (a pending entry, an XPENDING summary,
    // XACK -> 1) the group is created at id `0`, making the two existing entries
    // deliverable. Recorded follow-up: quickstart.md Step 4 `$`/`>` sequence.
    assert_eq!(q::<String>(&mut c, &["XGROUP", "CREATE", "mystream", "mygroup", "0", "MKSTREAM"]).await, "OK");

    let read = q::<Value>(
        &mut c,
        &["XREADGROUP", "GROUP", "mygroup", "consumer1", "COUNT", "1", "STREAMS", "mystream", ">"],
    )
    .await;
    assert_eq!(arr(&read).len(), 1, "one pending entry: {read:?}");

    let pending = q::<Value>(&mut c, &["XPENDING", "mystream", "mygroup"]).await;
    assert_eq!(arr(&pending).len(), 4, "XPENDING summary: {pending:?}");

    assert_eq!(q::<i64>(&mut c, &["XACK", "mystream", "mygroup", id1.as_str()]).await, 1);

    lab.stop();
}

// ---------------------------------------------------------------------------
// Step 5: Blocking Commands
// ---------------------------------------------------------------------------

#[tokio::test]
async fn quickstart_step5_blocking() {
    let lab = boot_redis_complete_product().await;
    let mut c1 = lab.connect().await;
    let mut c2 = lab.connect().await;

    // Release-on-arrival: a second connection pushes after a short delay.
    let pusher = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        let _: i64 = q(&mut c2, &["LPUSH", "waitlist", "released"]).await;
    });
    let r: Option<(String, String)> = q(&mut c1, &["BLPOP", "waitlist", "3"]).await;
    pusher.await.unwrap();
    assert_eq!(r, Some(("waitlist".to_string(), "released".to_string())));

    // Timeout path (≤ 1 s per the guide): empty key → nil.
    let r: Option<(String, String)> = q(&mut c1, &["BLPOP", "emptylist", "1"]).await;
    assert!(r.is_none(), "BLPOP timeout must be nil");

    lab.stop();
}

// ---------------------------------------------------------------------------
// Step 6: Transactions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn quickstart_step6_transactions() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    assert_eq!(bulk(&q(&mut c, &["MULTI"]).await), "OK");
    assert_eq!(bulk(&q(&mut c, &["SET", "txkey", "value"]).await), "QUEUED");
    assert_eq!(bulk(&q(&mut c, &["INCR", "txcounter"]).await), "QUEUED");
    let exec = q::<Value>(&mut c, &["EXEC"]).await;
    let items = arr(&exec);
    assert_eq!(items.len(), 2, "EXEC multi-bulk: {exec:?}");
    assert_eq!(bulk(&items[0]), "OK");
    assert_eq!(int(&items[1]), 1);

    // WATCH without contention commits.
    assert_eq!(bulk(&q(&mut c, &["SET", "watchedkey", "1"]).await), "OK");
    assert_eq!(bulk(&q(&mut c, &["WATCH", "watchedkey"]).await), "OK");
    assert_eq!(bulk(&q(&mut c, &["MULTI"]).await), "OK");
    assert_eq!(bulk(&q(&mut c, &["INCR", "watchedkey"]).await), "QUEUED");
    let exec = q::<Value>(&mut c, &["EXEC"]).await;
    assert_eq!(arr(&exec).len(), 1);
    assert_eq!(int(&arr(&exec)[0]), 2);

    // WATCH + concurrent modification → null (aborted) EXEC.
    let mut c1 = lab.connect().await;
    let mut c2 = lab.connect().await;
    assert_eq!(bulk(&q(&mut c1, &["SET", "contended", "1"]).await), "OK");
    assert_eq!(bulk(&q(&mut c1, &["WATCH", "contended"]).await), "OK");
    // Concurrency in two connections: the modification lands after WATCH.
    assert_eq!(bulk(&q(&mut c2, &["SET", "contended", "2"]).await), "OK");
    assert_eq!(bulk(&q(&mut c1, &["MULTI"]).await), "OK");
    assert_eq!(bulk(&q(&mut c1, &["INCR", "contended"]).await), "QUEUED");
    let exec = q::<Value>(&mut c1, &["EXEC"]).await;
    assert!(is_nil(&exec), "WATCH abort must be nil, got {exec:?}");

    lab.stop();
}

// ---------------------------------------------------------------------------
// Step 7: Pub/Sub
// ---------------------------------------------------------------------------

#[tokio::test]
async fn quickstart_step7_pubsub() {
    let lab = boot_redis_complete_product().await;
    let mut publisher = lab.connect().await;

    let mut sub = pubsub_client(&lab)
        .get_async_pubsub()
        .await
        .expect("pubsub connect");
    sub.subscribe("news").await.expect("subscribe news");

    assert_eq!(int(&q(&mut publisher, &["PUBLISH", "news", "hello subscribers"]).await), 1);
    let msg = tokio::time::timeout(Duration::from_secs(5), sub.on_message().next())
        .await
        .expect("message timeout")
        .expect("stream ended");
    assert_eq!(msg.get_channel::<String>().unwrap(), "news");
    assert_eq!(msg.get_payload::<String>().unwrap(), "hello subscribers");

    sub.unsubscribe("news").await.expect("unsubscribe news");

    lab.stop();
}

// ---------------------------------------------------------------------------
// Step 8: HyperLogLog
// ---------------------------------------------------------------------------

#[tokio::test]
async fn quickstart_step8_hyperloglog() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    assert_eq!(q::<i64>(&mut c, &["PFADD", "visitors", "user1", "user2", "user3"]).await, 1);
    let n = q::<i64>(&mut c, &["PFCOUNT", "visitors"]).await;
    assert!((2..=5).contains(&n), "PFCOUNT after 3 elements: {n}");

    // Re-adding user2 changes nothing; user4 is new → 1.
    assert_eq!(q::<i64>(&mut c, &["PFADD", "visitors", "user2", "user4"]).await, 1);
    let n = q::<i64>(&mut c, &["PFCOUNT", "visitors"]).await;
    assert!((3..=6).contains(&n), "PFCOUNT after 4 elements: {n}");

    assert_eq!(q::<i64>(&mut c, &["PFADD", "more_visitors", "user5", "user6"]).await, 1);
    assert_eq!(
        q::<String>(&mut c, &["PFMERGE", "all_visitors", "visitors", "more_visitors"]).await,
        "OK"
    );
    let n = q::<i64>(&mut c, &["PFCOUNT", "all_visitors"]).await;
    assert!((5..=7).contains(&n), "PFCOUNT union of 6 elements: {n}");
    assert_eq!(q::<String>(&mut c, &["TYPE", "visitors"]).await, "string");

    lab.stop();
}

// ---------------------------------------------------------------------------
// Step 9: Bitmaps
// ---------------------------------------------------------------------------

#[tokio::test]
async fn quickstart_step9_bitmaps() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    assert_eq!(q::<i64>(&mut c, &["SETBIT", "mybitmap", "100", "1"]).await, 0);
    assert_eq!(q::<i64>(&mut c, &["GETBIT", "mybitmap", "100"]).await, 1);
    assert_eq!(q::<i64>(&mut c, &["GETBIT", "mybitmap", "99"]).await, 0);
    assert_eq!(q::<i64>(&mut c, &["BITCOUNT", "mybitmap"]).await, 1);
    assert_eq!(q::<i64>(&mut c, &["SETBIT", "mybitmap", "200", "1"]).await, 0);
    assert_eq!(q::<i64>(&mut c, &["BITCOUNT", "mybitmap"]).await, 2);

    // BITOP AND bm1 bm2 = only bit 1 set in both.
    assert_eq!(q::<i64>(&mut c, &["SETBIT", "bm1", "0", "1"]).await, 0);
    assert_eq!(q::<i64>(&mut c, &["SETBIT", "bm1", "1", "1"]).await, 0);
    assert_eq!(q::<i64>(&mut c, &["SETBIT", "bm2", "1", "1"]).await, 0);
    assert_eq!(q::<i64>(&mut c, &["SETBIT", "bm2", "2", "1"]).await, 0);
    assert_eq!(q::<i64>(&mut c, &["BITOP", "AND", "result", "bm1", "bm2"]).await, 1);
    assert_eq!(q::<i64>(&mut c, &["BITCOUNT", "result"]).await, 1);

    assert_eq!(q::<String>(&mut c, &["TYPE", "mybitmap"]).await, "string");

    lab.stop();
}

// ---------------------------------------------------------------------------
// Step 10: INFO Command
// ---------------------------------------------------------------------------

#[tokio::test]
async fn quickstart_step10_info() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    assert_eq!(q::<String>(&mut c, &["SET", "infokey", "v"]).await, "OK");

    let server: String = q(&mut c, &["INFO", "server"]).await;
    assert!(server.contains("# Server"), "INFO server: {server}");
    assert!(server.contains("redis_version"), "INFO server: {server}");

    let keyspace: String = q(&mut c, &["INFO", "keyspace"]).await;
    assert!(!keyspace.is_empty(), "INFO keyspace must be non-empty");

    assert!(q::<i64>(&mut c, &["DBSIZE"]).await >= 1);

    lab.stop();
}

// ---------------------------------------------------------------------------
// Step 11: Not-Supported Commands
// ---------------------------------------------------------------------------

#[tokio::test]
async fn quickstart_step11_not_supported() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    let e = qerr(&mut c, &["GEOADD", "mygeo", "13.361389", "38.115556", "Palermo"]).await;
    assert!(e.contains("unknown command 'GEOADD'"), "GEOADD -> {e}");

    let e = qerr(&mut c, &["EVAL", "return redis.call('GET', KEYS[1])", "1", "mykey"]).await;
    assert!(e.contains("unknown command 'EVAL'"), "EVAL -> {e}");

    // CLUSTER NODES may reply "disabled" info or the unknown-command error.
    match run(&mut c, &["CLUSTER", "NODES"]).await {
        Ok(Value::SimpleString(s)) => assert!(!s.is_empty() && s != "OK", "CLUSTER NODES -> {s:?}"),
        Ok(Value::BulkString(b)) => assert!(!b.is_empty(), "CLUSTER NODES -> empty bulk"),
        Ok(other) => panic!("CLUSTER NODES unexpected success {other:?}"),
        Err(e) => assert!(e.contains("unknown command 'CLUSTER'"), "CLUSTER NODES -> {e}"),
    }

    let e = qerr(&mut c, &["ACL", "LIST"]).await;
    assert!(e.contains("unknown command 'ACL'"), "ACL LIST -> {e}");

    lab.stop();
}

// ---------------------------------------------------------------------------
// Next Steps #2: first-binary negative check
// ---------------------------------------------------------------------------

/// No complete-product row leaks into the `016` first-binary K/V profile: the
/// new MUST verbs stay errors (`SC-006` / `FR-005`).
#[tokio::test]
async fn quickstart_first_binary_negative() {
    let lab = boot_redis_with_profile(DialectProfile::FirstBinary).await;
    let mut c = lab.connect().await;

    for parts in [
        &["HGET", "k", "f"][..],
        &["XADD", "k", "*", "f", "v"][..],
        &["PFADD", "k", "a"][..],
        &["SETBIT", "k", "0", "1"][..],
        &["INFO"][..],
    ] {
        let e = qerr(&mut c, parts).await;
        assert!(e.contains("unknown command"), "{parts:?} on FirstBinary -> {e}");
    }

    // The first-binary K/V MUST list still works on the same connection.
    assert_eq!(q::<String>(&mut c, &["SET", "k", "v"]).await, "OK");
    assert_eq!(q::<String>(&mut c, &["GET", "k"]).await, "v");

    lab.stop();
}

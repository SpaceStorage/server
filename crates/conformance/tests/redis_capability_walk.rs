//! `017` T070 — full-capability-table conformance walk (SC-001).
//!
//! Boots ONE complete-product lab and drives every row of
//! [`REDIS_CAPABILITY_TABLE`] through an unmodified stock `redis` client, so the
//! walk exercises the real RESP wire path of the whole dialect rather than an
//! internal model. The row list is taken from the single source of truth, so the
//! test cannot drift from `contracts/capability-table.md`.
//!
//! Two assertion strengths are used, and the distinction is deliberate:
//!
//! * **Reachability** — every row must be *dispatched*: the reply must not be the
//!   classifier's fall-through `-ERR unknown command '<BASE>'`. A genuine
//!   protocol error from the handler (arity, `WRONGTYPE`, `NOGROUP`,
//!   `EXECABORT`, `BUSYGROUP`, …) is an acceptable outcome, because it proves the
//!   row reached its handler rather than being rejected as unknown.
//! * **Round-trip** — the rows below additionally assert a specific decoded
//!   reply, i.e. a real read-back of a value the command wrote:
//!   `SET`/`GET`, `HSET`/`HGET`, `XADD`/`XLEN`, `SETBIT`/`GETBIT`,
//!   `PFADD`/`PFCOUNT`, `INFO`, `TYPE`, `DBSIZE`, `COMMAND`, `CLIENT ID`,
//!   `CONFIG GET max_block`, `OBJECT ENCODING`.
//!
//! Rows whose command changes connection state (`QUIT`,
//! `MULTI`/`EXEC`/`DISCARD`/`WATCH`/`UNWATCH`, the subscribe family,
//! `CONFIG SET`) run on a fresh ephemeral connection so the long-lived walk
//! connection stays in ordinary command mode and the walk outcome is order-free.
#![cfg(feature = "redis-dialect")]

mod support;

use std::time::Duration;

use redis::Value;
use spacestorage_compat::REDIS_CAPABILITY_TABLE;

use support::redis_live::{boot_redis_complete_product, RedisLab};

type Conn = redis::aio::MultiplexedConnection;

/// Hard deadline for one row. Blocking rows are pre-seeded so they return at
/// once; a parked row that never returns fails loudly instead of hanging.
const ROW_DEADLINE: Duration = Duration::from_secs(5);

/// Run `parts` (command + string args) on a stock client.
async fn q<T: redis::FromRedisValue>(c: &mut Conn, parts: &[&str]) -> T {
    let mut cmd = redis::cmd(parts[0]);
    for p in &parts[1..] {
        cmd.arg(*p);
    }
    cmd.query_async(c)
        .await
        .unwrap_or_else(|e| panic!("{parts:?} -> {e}"))
}

/// Run `parts` under a hard deadline, mapping a server error to its rendered
/// text. Used by the walk so a reachability miss is a normal `Err`, not a panic.
async fn run(c: &mut Conn, parts: &[&str]) -> Result<Value, String> {
    let mut cmd = redis::cmd(parts[0]);
    for p in &parts[1..] {
        cmd.arg(*p);
    }
    match tokio::time::timeout(ROW_DEADLINE, cmd.query_async::<Value>(c)).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => panic!("{parts:?} hung past {ROW_DEADLINE:?}"),
    }
}

fn as_int(v: &Value) -> i64 {
    match v {
        Value::Int(n) => *n,
        other => panic!("expected int, got {other:?}"),
    }
}

fn as_arr(v: &Value) -> &[Value] {
    match v {
        Value::Array(a) => a,
        other => panic!("expected array, got {other:?}"),
    }
}

/// The base (first) token of a capability row name.
fn base_of(name: &str) -> &str {
    name.split(' ').next().unwrap_or(name)
}

/// Rows that leave the connection in a different state after they run. They get
/// a fresh ephemeral connection so the walk connection is not disturbed.
const EPHEMERAL: &[&str] = &[
    "QUIT",
    "SUBSCRIBE",
    "UNSUBSCRIBE",
    "PSUBSCRIBE",
    "PUNSUBSCRIBE",
    "SSUBSCRIBE",
    "SUNSUBSCRIBE",
    "CONFIG SET",
    "MULTI",
    "EXEC",
    "DISCARD",
    "WATCH",
    "UNWATCH",
];

/// The full argv for one capability row: the row name split on spaces, plus the
/// minimal arguments that let the row reach its handler. Exhaustive by design —
/// a missing arm panics so no row can be silently skipped.
fn invocation(name: &'static str) -> Vec<&'static str> {
    let mut argv: Vec<&'static str> = name.split(' ').collect();
    argv.extend(extra_args(name));
    argv
}

fn extra_args(name: &str) -> Vec<&'static str> {
    match name {
        // ---- R1: connection / server meta ----
        "AUTH" => vec!["demo", "demo"],
        "PING" => vec![],
        "ECHO" => vec!["hi"],
        "QUIT" => vec![],
        "SELECT" => vec!["0"],
        "COMMAND" => vec![],
        "COMMAND INFO" => vec![],
        "COMMAND COUNT" => vec![],
        "CLIENT" => vec!["INFO"],
        "CLIENT ID" => vec![],
        "CLIENT INFO" => vec![],
        "CLIENT LIST" => vec![],
        "CLIENT KILL" => vec![],
        "CLIENT SETNAME" => vec!["walk"],
        "CLIENT GETNAME" => vec![],
        "CLIENT PAUSE" => vec!["0"],
        "CONFIG GET" => vec!["max_block"],
        "CONFIG SET" => vec!["max_block", "0"],
        "TIME" => vec![],
        // ---- R2: keyspace introspection ----
        "INFO" => vec![],
        "DBSIZE" => vec![],
        "TYPE" => vec!["walk:seed"],
        "TTL" => vec!["walk:seed"],
        "PTTL" => vec!["walk:seed"],
        "EXISTS" => vec!["walk:seed"],
        "SCAN" => vec!["0"],
        "RANDOMKEY" => vec![],
        "OBJECT ENCODING" => vec!["walk:seed"],
        "OBJECT IDLETIME" => vec!["walk:seed"],
        "TOUCH" => vec!["walk:seed"],
        "UNLINK" => vec!["walk:unlink"],
        // ---- R3: strings + expiry ----
        "GET" => vec!["walk:seed"],
        "SET" => vec!["walk:set", "v"],
        "DEL" => vec!["walk:set"],
        "MGET" => vec!["walk:seed", "walk:absent"],
        "MSET" => vec!["walk:ma", "1", "walk:mb", "2"],
        "MSETNX" => vec!["walk:mc", "1", "walk:md", "2"],
        "SETNX" => vec!["walk:snx", "v"],
        "SETEX" => vec!["walk:setex", "100", "v"],
        "PSETEX" => vec!["walk:psetex", "100000", "v"],
        "GETSET" => vec!["walk:gs", "v"],
        "STRLEN" => vec!["walk:seed"],
        "APPEND" => vec!["walk:app", "x"],
        "GETRANGE" => vec!["walk:seed", "0", "-1"],
        "SETRANGE" => vec!["walk:sr", "0", "x"],
        "INCR" => vec!["walk:incr"],
        "DECR" => vec!["walk:decr"],
        "INCRBY" => vec!["walk:incrby", "5"],
        "DECRBY" => vec!["walk:decrby", "5"],
        "INCRBYFLOAT" => vec!["walk:incrf", "1.5"],
        "GETDEL" => vec!["walk:getdel"],
        "GETEX" => vec!["walk:seed"],
        "EXPIRE" => vec!["walk:seed", "100"],
        "PEXPIRE" => vec!["walk:seed", "100000"],
        "EXPIREAT" => vec!["walk:seed", "4000000000"],
        "PEXPIREAT" => vec!["walk:seed", "4000000000000"],
        "PERSIST" => vec!["walk:seed"],
        "EXPIRETIME" => vec!["walk:seed"],
        "PEXPIRETIME" => vec!["walk:seed"],
        // ---- R4: hash ----
        "HSET" => vec!["walk:h", "f", "v"],
        "HGET" => vec!["walk:h", "f"],
        "HMSET" => vec!["walk:h", "g", "v"],
        "HMGET" => vec!["walk:h", "f", "g"],
        "HGETALL" => vec!["walk:h"],
        "HKEYS" => vec!["walk:h"],
        "HVALS" => vec!["walk:h"],
        "HLEN" => vec!["walk:h"],
        "HEXISTS" => vec!["walk:h", "f"],
        "HDEL" => vec!["walk:h", "g"],
        "HSETNX" => vec!["walk:h", "h1", "v"],
        "HINCRBY" => vec!["walk:h", "n", "1"],
        "HINCRBYFLOAT" => vec!["walk:h", "nf", "1.5"],
        "HSTRLEN" => vec!["walk:h", "f"],
        "HRANDFIELD" => vec!["walk:h"],
        "HSCAN" => vec!["walk:h", "0"],
        "HEXPIRE" => vec!["walk:h", "100", "FIELDS", "1", "f"],
        "HPEXPIRE" => vec!["walk:h", "100000", "FIELDS", "1", "f"],
        "HEXPIRETIME" => vec!["walk:h", "FIELDS", "1", "f"],
        "HPEXPIRETIME" => vec!["walk:h", "FIELDS", "1", "f"],
        "HEXPIREAT" => vec!["walk:h", "4000000000", "FIELDS", "1", "f"],
        "HPEXPIREAT" => vec!["walk:h", "4000000000000", "FIELDS", "1", "f"],
        "HTTL" => vec!["walk:h", "FIELDS", "1", "f"],
        "HPTTL" => vec!["walk:h", "FIELDS", "1", "f"],
        "HGETEX" => vec!["walk:h", "FIELDS", "1", "f"],
        "HGETDEL" => vec!["walk:h", "FIELDS", "1", "h1"],
        // ---- R5: list ----
        "LPUSH" => vec!["walk:l", "a"],
        "RPUSH" => vec!["walk:l", "b"],
        "LPOP" => vec!["walk:l"],
        "RPOP" => vec!["walk:l"],
        "LPUSHX" => vec!["walk:lnone", "x"],
        "RPUSHX" => vec!["walk:lnone", "x"],
        "LLEN" => vec!["walk:l"],
        "LRANGE" => vec!["walk:l", "0", "-1"],
        "LINDEX" => vec!["walk:l", "0"],
        "LSET" => vec!["walk:l", "0", "z"],
        "LREM" => vec!["walk:l", "0", "a"],
        "LTRIM" => vec!["walk:l", "0", "0"],
        "LINSERT" => vec!["walk:l", "BEFORE", "b", "x"],
        "RPOPLPUSH" => vec!["walk:l", "walk:ldst"],
        "BLPOP" => vec!["walk:blk1", "1"],
        "BRPOP" => vec!["walk:blk2", "1"],
        "BRPOPLPUSH" => vec!["walk:blk3", "walk:ldst2", "1"],
        "LMOVE" => vec!["walk:l2", "walk:ldst3", "LEFT", "RIGHT"],
        "BLMOVE" => vec!["walk:blk4", "walk:ldst4", "LEFT", "LEFT", "1"],
        // ---- R6: set ----
        "SADD" => vec!["walk:s", "a", "b"],
        "SREM" => vec!["walk:s", "a"],
        "SISMEMBER" => vec!["walk:s", "b"],
        "SMEMBERS" => vec!["walk:s"],
        "SCARD" => vec!["walk:s"],
        "SPOP" => vec!["walk:s"],
        "SRANDMEMBER" => vec!["walk:s"],
        "SSCAN" => vec!["walk:s", "0"],
        "SUNION" => vec!["walk:s", "walk:s2"],
        "SUNIONSTORE" => vec!["walk:sd1", "walk:s", "walk:s2"],
        "SINTER" => vec!["walk:s", "walk:s2"],
        "SINTERSTORE" => vec!["walk:sd2", "walk:s", "walk:s2"],
        "SDIFF" => vec!["walk:s", "walk:s2"],
        "SDIFFSTORE" => vec!["walk:sd3", "walk:s", "walk:s2"],
        // ---- R7: sorted set ----
        "ZADD" => vec!["walk:z", "1", "a", "2", "b"],
        "ZREM" => vec!["walk:z", "a"],
        "ZCARD" => vec!["walk:z"],
        "ZSCORE" => vec!["walk:z", "b"],
        "ZMSCORE" => vec!["walk:z", "a", "b"],
        "ZCOUNT" => vec!["walk:z", "0", "10"],
        "ZRANK" => vec!["walk:z", "b"],
        "ZREVRANK" => vec!["walk:z", "b"],
        "ZRANGE" => vec!["walk:z", "0", "-1"],
        "ZREVRANGE" => vec!["walk:z", "0", "-1"],
        "ZRANGEBYSCORE" => vec!["walk:z", "0", "10"],
        "ZREVRANGEBYSCORE" => vec!["walk:z", "10", "0"],
        "ZPOPMIN" => vec!["walk:z"],
        "ZPOPMAX" => vec!["walk:z"],
        "BZPOPMIN" => vec!["walk:zblk1", "1"],
        "BZPOPMAX" => vec!["walk:zblk2", "1"],
        "ZUNIONSTORE" => vec!["walk:zd1", "2", "walk:z", "walk:z2"],
        "ZINTERSTORE" => vec!["walk:zd2", "2", "walk:z", "walk:z2"],
        "ZDIFFSTORE" => vec!["walk:zd3", "2", "walk:z", "walk:z2"],
        "ZUNION" => vec!["2", "walk:z", "walk:z2"],
        "ZINTER" => vec!["2", "walk:z", "walk:z2"],
        "ZDIFF" => vec!["2", "walk:z", "walk:z2"],
        "ZSCAN" => vec!["walk:z", "0"],
        "ZINCRBY" => vec!["walk:z", "1", "b"],
        "ZREMRANGEBYSCORE" => vec!["walk:z", "0", "0"],
        "ZREMRANGEBYRANK" => vec!["walk:z", "0", "0"],
        "ZRANGESTORE" => vec!["walk:zrs", "walk:z", "0", "-1"],
        // ---- R8: streams ----
        "XADD" => vec!["walk:st", "*", "f", "v"],
        "XDEL" => vec!["walk:st", "0-0"],
        "XRANGE" => vec!["walk:st", "-", "+"],
        "XREVRANGE" => vec!["walk:st", "+", "-"],
        "XLEN" => vec!["walk:st"],
        "XTRIM" => vec!["walk:st", "MAXLEN", "100"],
        "XREAD" => vec!["STREAMS", "walk:st", "0"],
        "XREADGROUP" => vec!["GROUP", "walk:g", "c", "STREAMS", "walk:st", ">"],
        "XGROUP" => vec!["CREATE", "walk:stg", "walk:g", "$", "MKSTREAM"],
        "XGROUP CREATE" => vec!["walk:stg2", "walk:g", "$", "MKSTREAM"],
        "XGROUP DESTROY" => vec!["walk:st", "walk:gdel"],
        "XGROUP SETID" => vec!["walk:stg", "walk:g", "0"],
        "XGROUP DELCONSUMER" => vec!["walk:stg", "walk:g", "c"],
        "XGROUP CREATECONSUMER" => vec!["walk:stg", "walk:g", "c2"],
        "XGROUP HELP" => vec![],
        "XACK" => vec!["walk:st", "walk:g", "1-1"],
        "XPENDING" => vec!["walk:st", "walk:g"],
        "XCLAIM" => vec!["walk:st", "walk:g", "c", "0", "0-0"],
        "XINFO" => vec!["STREAM", "walk:st"],
        "XINFO STREAM" => vec!["walk:st"],
        "XINFO GROUPS" => vec!["walk:st"],
        "XINFO CONSUMERS" => vec!["walk:st", "walk:g"],
        // ---- R9: bitmaps ----
        "SETBIT" => vec!["walk:bm", "7", "1"],
        "GETBIT" => vec!["walk:bm", "7"],
        "BITCOUNT" => vec!["walk:bm"],
        "BITPOS" => vec!["walk:bm", "1"],
        "BITOP" => vec!["AND", "walk:bmd", "walk:bm", "walk:bm"],
        "BITFIELD" => vec!["walk:bf", "SET", "u8", "0", "1"],
        "BITFIELD_RO" => vec!["walk:bf", "GET", "u8", "0"],
        // ---- R9: HyperLogLog ----
        "PFADD" => vec!["walk:hll", "a", "b", "c"],
        "PFCOUNT" => vec!["walk:hll"],
        "PFMERGE" => vec!["walk:hll2", "walk:hll"],
        // ---- Pub/Sub ----
        "SUBSCRIBE" => vec!["walk-ch"],
        "UNSUBSCRIBE" => vec![],
        "PSUBSCRIBE" => vec!["walk.*"],
        "PUNSUBSCRIBE" => vec![],
        "PUBLISH" => vec!["walk-pub", "x"],
        "PUBSUB" => vec!["CHANNELS"],
        "SPUBLISH" => vec!["walk-pub", "x"],
        "SSUBSCRIBE" => vec!["walk-sh"],
        "SUNSUBSCRIBE" => vec![],
        // ---- Transactions ----
        "MULTI" => vec![],
        "EXEC" => vec![],
        "DISCARD" => vec![],
        "WATCH" => vec!["walk:seed"],
        "UNWATCH" => vec![],
        other => panic!("no invocation mapped for capability row {other:?}"),
    }
}

/// Boot one lab, pre-seed the keys the rows read, then require every capability
/// row to be *dispatched* (never `unknown command '<BASE>'`).
#[tokio::test]
async fn capability_table_walk_reaches_every_row() {
    let lab: RedisLab = boot_redis_complete_product().await;
    let mut walk = lab.connect().await;

    // Pre-seed: string, list, set, zset, hash, stream (+ group), HLL, and the
    // blocking source keys so a parked row returns immediately.
    for parts in [
        &["SET", "walk:seed", "v"][..],
        &["RPUSH", "walk:l", "a", "b", "c"][..],
        &["RPUSH", "walk:l2", "a", "b", "c"][..],
        &["RPUSH", "walk:blk1", "a"][..],
        &["RPUSH", "walk:blk2", "a"][..],
        &["RPUSH", "walk:blk3", "a"][..],
        &["RPUSH", "walk:blk4", "a"][..],
        &["ZADD", "walk:z", "1", "a", "2", "b"][..],
        &["ZADD", "walk:z2", "2", "b", "3", "c"][..],
        &["ZADD", "walk:zblk1", "1", "a"][..],
        &["ZADD", "walk:zblk2", "1", "a"][..],
        &["SADD", "walk:s", "a", "b", "c"][..],
        &["SADD", "walk:s2", "b", "c", "d"][..],
        &["HSET", "walk:h", "f", "v"][..],
        &["XADD", "walk:st", "1-1", "f", "v"][..],
        &["XADD", "walk:st", "2-2", "f", "v"][..],
        &["XGROUP", "CREATE", "walk:st", "walk:g", "0"][..],
        &["XGROUP", "CREATE", "walk:st", "walk:gdel", "0"][..],
        &["PFADD", "walk:hll", "a", "b", "c"][..],
    ] {
        let _: Value = q(&mut walk, parts).await;
    }

    let mut walked = 0usize;
    let mut reachability_only: Vec<&str> = Vec::new();
    for row in REDIS_CAPABILITY_TABLE {
        let name = row.name;
        let base = base_of(name);
        let parts = invocation(name);
        let result = if EPHEMERAL.contains(&name) {
            let mut eph = lab.connect().await;
            run(&mut eph, &parts).await
        } else {
            run(&mut walk, &parts).await
        };
        if let Err(e) = result {
            // The only disallowed reply is the classifier fall-through that says
            // the row's base command is unknown. Any other protocol error means
            // the row reached its handler.
            let forbidden = format!("unknown command '{base}'");
            assert!(
                !e.contains(&forbidden),
                "row `{name}` was not dispatched: {e}"
            );
            reachability_only.push(name);
        }
        walked += 1;
    }

    assert_eq!(
        walked,
        REDIS_CAPABILITY_TABLE.len(),
        "every capability row must be walked exactly once"
    );
    assert_eq!(walked, 191, "capability table size (contracts/capability-table.md)");

    // The following rows were driven above; assert the real read-back round-trips
    // promised by SC-001 (0 failures).
    let mut c = lab.connect().await;

    // SET / GET.
    assert_eq!(q::<String>(&mut c, &["SET", "walk:rt", "hello"]).await, "OK");
    assert_eq!(q::<Option<String>>(&mut c, &["GET", "walk:rt"]).await.as_deref(), Some("hello"));

    // HSET / HGET.
    assert_eq!(q::<i64>(&mut c, &["HSET", "walk:rth", "f", "v"]).await, 1);
    assert_eq!(q::<Option<String>>(&mut c, &["HGET", "walk:rth", "f"]).await.as_deref(), Some("v"));

    // XADD / XLEN.
    let _: String = q(&mut c, &["XADD", "walk:rts", "*", "f", "v"]).await;
    assert_eq!(q::<i64>(&mut c, &["XLEN", "walk:rts"]).await, 1);

    // SETBIT / GETBIT.
    assert_eq!(q::<i64>(&mut c, &["SETBIT", "walk:rtb", "100", "1"]).await, 0);
    assert_eq!(q::<i64>(&mut c, &["GETBIT", "walk:rtb", "100"]).await, 1);

    // PFADD / PFCOUNT.
    assert_eq!(q::<i64>(&mut c, &["PFADD", "walk:rthll", "a", "b", "c"]).await, 1);
    assert_eq!(q::<i64>(&mut c, &["PFCOUNT", "walk:rthll"]).await, 3);

    // INFO.
    let info: String = q(&mut c, &["INFO"]).await;
    assert!(info.contains("# Server"), "INFO server section: {info}");

    // TYPE / OBJECT ENCODING.
    assert_eq!(q::<String>(&mut c, &["TYPE", "walk:rt"]).await, "string");
    assert_eq!(q::<String>(&mut c, &["OBJECT", "ENCODING", "walk:rt"]).await, "raw");

    // DBSIZE.
    assert!(q::<i64>(&mut c, &["DBSIZE"]).await >= 1);

    // COMMAND.
    let listed: Vec<Value> = q(&mut c, &["COMMAND"]).await;
    assert_eq!(listed.len(), REDIS_CAPABILITY_TABLE.len());

    // CLIENT ID.
    assert!(q::<i64>(&mut c, &["CLIENT", "ID"]).await > 0);

    // CONFIG GET max_block (after CONFIG SET on a fresh connection).
    let mut cfg = lab.connect().await;
    assert_eq!(q::<String>(&mut cfg, &["CONFIG", "SET", "max_block", "5"]).await, "OK");
    let got: Vec<String> = q(&mut cfg, &["CONFIG", "GET", "max_block"]).await;
    assert_eq!(got, vec!["max_block".to_string(), "5".to_string()]);

    eprintln!(
        "capability walk: {walked} rows, {} reachability-only (handler protocol errors): {:?}",
        reachability_only.len(),
        reachability_only
    );

    lab.stop();
}

/// The walk's arm table must cover the table exactly — a rename/append that the
/// map does not know about fails here instead of being silently skipped.
#[test]
fn capability_table_invocations_are_exhaustive() {
    for row in REDIS_CAPABILITY_TABLE {
        let argv = invocation(row.name);
        assert!(
            !argv.is_empty(),
            "row {} produced an empty argv",
            row.name
        );
    }
    // Table size is pinned by the contract (191 rows); the walk asserts the same
    // value at runtime, this keeps the constant honest if the table changes.
    assert_eq!(REDIS_CAPABILITY_TABLE.len(), 191);
}

//! `017` T063 + T065: the closed not-supported list and the cross-protocol
//! shared-object promise, against an unmodified stock Redis client.
//!
//! T063 (SC-002): every command in
//! [`contracts/not-supported.md`](../../../specs/017-redis-dialect-coverage/contracts/not-supported.md)
//! returns a Redis error and never a `+OK`/empty-array success. The two documented
//! exceptions are honoured: `SELECT n>0` keeps the `002` form
//! (`SELECT is not supported: namespaces are bound to credentials`) and
//! `CLUSTER INFO`/`CLUSTER NODES` may reply "disabled" info instead of erroring.
//!
//! T065 (FR-003 / `002` shared-object promise): a string written with Redis `SET`
//! is read back through the canonical `SS.GET`/PostgreSQL point read on the same
//! container, and the entry type is visible from that canonical path — proving
//! one shared store object rather than a protocol-private store.
#![cfg(feature = "redis-dialect")]

mod support;

use std::time::Duration;

use redis::Value;
use support::redis_live::{boot_redis_complete_product, RedisLab};

type Conn = redis::aio::MultiplexedConnection;

/// Run `parts` on a stock client, mapping any server error to its rendered text.
async fn run(c: &mut Conn, parts: &[&str]) -> Result<Value, String> {
    let mut cmd = redis::cmd(parts[0]);
    for p in &parts[1..] {
        cmd.arg(*p);
    }
    cmd.query_async::<Value>(c).await.map_err(|e| e.to_string())
}

/// Run `parts` with a hard deadline so a hang fails the test instead of blocking.
async fn run_timed(c: &mut Conn, parts: &[&str]) -> Result<Value, String> {
    tokio::time::timeout(Duration::from_secs(5), run(c, parts))
        .await
        .unwrap_or_else(|_| panic!("{parts:?} hung past 5s"))
}

async fn expect_error(c: &mut Conn, parts: &[&str]) -> String {
    match run_timed(c, parts).await {
        Ok(v) => panic!("{parts:?} must error, got {v:?}"),
        Err(e) => e,
    }
}

/// Every command row of `contracts/not-supported.md` (§1–§9, §11) that takes the
/// generic `-ERR unknown command '<COMMAND>'` form. The two-word CLUSTER rows and
/// the §10 RESP3/`SELECT`/`OBJECT`/`CONFIG` special forms are covered separately.
const UNKNOWN_CMD_CASES: &[&[&str]] = &[
    // §1 Geospatial
    &["GEOADD", "k", "1", "2", "m"],
    &["GEOPOS", "k", "m"],
    &["GEODIST", "k", "m1", "m2"],
    &["GEORADIUS", "k", "1", "2", "m"],
    &["GEORADIUS_RO", "k", "1", "2", "m"],
    &["GEORADIUSBYMEMBER", "k", "m", "1", "m"],
    &["GEORADIUSBYMEMBER_RO", "k", "m", "1", "m"],
    &["GEOSEARCH", "k", "FROMMEMBER", "m", "BYRADIUS", "1", "m"],
    &["GEOSEARCHSTORE", "d", "s", "FROMMEMBER", "m", "BYRADIUS", "1", "m"],
    &["GEOHASH", "k", "m"],
    // §2 Stream auto-claim
    &["XAUTOCLAIM", "s", "g", "c", "0", "0"],
    // §3 Lua / scripting
    &["EVAL", "return 1", "0"],
    &["EVALSHA", "deadbeef", "0"],
    &["SCRIPT", "LOAD", "return 1"],
    &["SCRIPT", "EXISTS", "deadbeef"],
    &["SCRIPT", "FLUSH"],
    &["SCRIPT", "KILL"],
    // §4 Functions
    &["FCALL", "f", "0"],
    &["FCALL_RO", "f", "0"],
    &["FUNCTION", "LIST"],
    &["FUNCTION", "STATS"],
    &["FUNCTION", "DUMP"],
    // §5 Modules
    &["MODULE", "LIST"],
    &["MODULE", "LOAD", "/x.so"],
    // §6 Cluster (all non-INFO/NODES subcommands)
    &["CLUSTER", "SLOTS"],
    &["CLUSTER", "KEYSLOT", "k"],
    &["CLUSTER", "ADDSLOTS", "1"],
    &["CLUSTER", "DELSLOTS", "1"],
    &["CLUSTER", "FLUSHSLOTS"],
    &["CLUSTER", "SETSLOT", "1", "NODE", "n"],
    &["CLUSTER", "BUMPEPOCH"],
    &["CLUSTER", "SAVECONFIG"],
    &["CLUSTER", "FORGET", "n"],
    &["CLUSTER", "REPLICATE", "n"],
    &["CLUSTER", "SLAVES", "n"],
    &["CLUSTER", "COUNT-FAILURE-REPORTS", "n"],
    &["CLUSTER", "FAILOVER"],
    // §7 Sentinel
    &["SENTINEL", "masters"],
    &["SENTINEL", "master", "m"],
    &["SENTINEL", "slaves", "m"],
    &["SENTINEL", "get-master-addr-by-name", "m"],
    &["SENTINEL", "flushconfig"],
    // §8 ACL
    &["ACL", "LIST"],
    &["ACL", "USERS"],
    &["ACL", "GETUSER", "u"],
    &["ACL", "SETUSER", "u"],
    &["ACL", "WHOAMI"],
    &["ACL", "CAT"],
    &["ACL", "GENPASS"],
    &["ACL", "HELP"],
    // §9 Historical / deprecated
    &["DEBUG", "OBJECT", "k"],
    &["DEBUG", "SEGFAULT"],
    &["DEBUG", "RELOAD"],
    &["DEBUG", "LOG"],
    &["DEBUG", "OOM"],
    &["SHUTDOWN"],
    &["SHUTDOWN", "NOSAVE"],
    &["SLAVEOF", "h", "1"],
    &["REPLICAOF", "h", "1"],
    &["SYNC"],
    &["PSYNC"],
    &["MIGRATE", "h", "1", "k", "0", "10"],
    &["RESTORE", "k", "0", "v"],
    &["DUMP", "k"],
    &["RENAME", "a", "b"],
    &["RENAMENX", "a", "b"],
    // §10 RESP3-only
    &["HELLO", "3"],
    // §11 Swap (SELECT n>0 is a documented exception, covered below)
    &["SWAPDB", "0", "1"],
];

/// T063 / SC-002: every documented not-supported command returns the Redis
/// error form and never a success.
#[tokio::test]
async fn complete_product_not_supported_list_never_succeeds() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    for case in UNKNOWN_CMD_CASES {
        let verb = case[0].to_ascii_uppercase();
        let expected = format!("unknown command '{verb}'");
        match run_timed(&mut c, case).await {
            Ok(v) => panic!("{case:?} must not succeed, got {v:?}"),
            Err(e) => assert!(
                e.contains(&expected),
                "{case:?} -> {e:?}, expected to contain {expected:?}"
            ),
        }
    }

    lab.stop();
}

/// §6 / §10 / §11 special forms: `CLUSTER INFO|NODES` (error or info, never a
/// bare `OK`), the `SELECT n>0` `002` error, and the bad-subcommand forms of the
/// listed two-word commands.
#[tokio::test]
async fn not_supported_special_forms_error_never_ok() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    // §6: CLUSTER INFO / NODES may reply "disabled" info; anything but a plain
    // success is acceptable, and a bare `+OK`/empty reply must not appear.
    for parts in [&["CLUSTER", "INFO"][..], &["CLUSTER", "NODES"][..]] {
        match run_timed(&mut c, parts).await {
            Ok(Value::SimpleString(s)) => {
                assert!(!s.is_empty() && s != "OK", "{parts:?} -> {s:?}");
            }
            Ok(Value::BulkString(b)) => {
                let s = String::from_utf8_lossy(&b);
                assert!(!s.is_empty(), "{parts:?} -> empty bulk");
            }
            Ok(other) => panic!("{parts:?} -> unexpected success {other:?}"),
            Err(e) => assert!(e.contains("unknown command 'CLUSTER'"), "{parts:?} -> {e:?}"),
        }
    }

    // §11: `SELECT 0` is a no-op MUST; `SELECT n>0` keeps the documented `002`
    // form (not the generic unknown-command form).
    assert!(
        matches!(run_timed(&mut c, &["SELECT", "0"]).await, Ok(Value::Okay)),
        "SELECT 0 must stay a no-op"
    );
    let e = expect_error(&mut c, &["SELECT", "7"]).await;
    assert!(e.contains("SELECT is not supported"), "SELECT 7 -> {e:?}");

    // §6 / §10 / §2 rows: a bad (or unsupported) subcommand of a listed command
    // is an error — never a silent success.
    for parts in [
        &["OBJECT", "FREQ", "k"][..],
        &["OBJECT", "REFCOUNT", "k"][..],
        &["OBJECT", "HELP"][..],
        &["CONFIG", "RESETSTAT"][..],
        &["CONFIG", "REWRITE"][..],
        &["CLIENT", "SETINFO", "lib-name", "x"][..],
    ] {
        let e = expect_error(&mut c, parts).await;
        let low = e.to_ascii_lowercase();
        assert!(
            low.contains("unknown"),
            "{parts:?} -> {e:?} (expected an unknown-command/subcommand error)"
        );
    }

    lab.stop();
}

/// T065 / FR-003: one shared key-space container. A string written via Redis
/// `SET` is the same object the canonical `SS.GET`/PostgreSQL point read
/// (`002` FR-035 `KeyGet`, [`spacestorage_query::catalog_exec::key_get`])
/// returns, and the entry's Redis-visible type is recoverable from that path.
#[tokio::test]
async fn cross_protocol_shared_object_is_visible() {
    let lab: RedisLab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    let value = b"test value";
    let ok: String = redis::cmd("SET")
        .arg("shared")
        .arg(value.as_slice())
        .query_async(&mut c)
        .await
        .expect("SET");
    assert_eq!(ok, "OK");

    // Canonical `SS.GET`/PostgreSQL point read of the same namespace container.
    let r = spacestorage_query::catalog_exec::key_get(&lab.catalog, "demo", "redis", "shared")
        .expect("key_get on the shared container");
    assert_eq!(r.tag, "GET 1", "the entry is present through the canonical path");
    let blob = r.blob.expect("canonical entry bytes");
    // The shared channel stores the canonical `lower::encode_entry` blob
    // (`redis_binary_safety.rs` pins this), not the raw value: a
    // `[kind, version, expire, len, value]` record. The value is the trailing
    // bytes and the leading kind byte is `0` (`EntryPayload::Bytes` → `string`).
    const KIND_BYTES: u8 = 0;
    assert_eq!(blob.first().copied(), Some(KIND_BYTES), "entry type is `string`");
    assert!(
        blob.ends_with(value),
        "the Redis value round-trips inside the shared entry blob"
    );

    // The Redis client still sees the same object with its Redis-visible type.
    let got: Vec<u8> = redis::cmd("GET")
        .arg("shared")
        .query_async(&mut c)
        .await
        .expect("GET");
    assert_eq!(got, value);
    let ty: String = redis::cmd("TYPE")
        .arg("shared")
        .query_async(&mut c)
        .await
        .expect("TYPE");
    assert_eq!(ty, "string");

    lab.stop();
}

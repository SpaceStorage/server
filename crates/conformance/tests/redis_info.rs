//! US4 (`017` T049): `INFO` and introspection conformance against an
//! unmodified stock Redis client.
//!
//! The live harness serves the real `RedisHandler` on an ephemeral loopback
//! port; every reply is decoded by the stock `redis` crate, so these pin the
//! RESP wire shape and the CRLF `# section` / `field:value` `INFO` format
//! rather than an internal model. Counters are asserted to move with real
//! operations (SC-006), and `INFO keyspace`/`DBSIZE` are asserted to match the
//! live, non-expired key-space.
#![cfg(feature = "redis-dialect")]

mod support;

use std::collections::BTreeMap;
use std::time::Duration;

use redis::Value;
use spacestorage_compat::REDIS_CAPABILITY_TABLE;

use support::redis_live::{boot_redis_complete_product, RedisLab};

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

async fn info(c: &mut Conn, args: &[&str]) -> String {
    let mut cmd = redis::cmd("INFO");
    for a in args {
        cmd.arg(*a);
    }
    cmd.query_async::<String>(c).await.expect("INFO reply")
}

/// `field:value` lookup inside one `INFO` section.
fn field(text: &str, name: &str) -> Option<String> {
    text.split("\r\n")
        .filter_map(|line| line.split_once(':'))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_string())
}

fn stats_snapshot(text: &str) -> BTreeMap<String, u64> {
    text.split("\r\n")
        .filter_map(|line| line.split_once(':'))
        .filter_map(|(k, v)| v.parse::<u64>().ok().map(|n| (k.to_string(), n)))
        .collect()
}

/// Parse the `db0:keys=<n>,expires=<n>,avg_ttl=0` line of `INFO keyspace`.
fn keyspace_counts(text: &str) -> (u64, u64) {
    let line = text
        .split("\r\n")
        .find(|l| l.starts_with("db0:"))
        .unwrap_or_else(|| panic!("no db0 line in {text:?}"));
    let payload = line.trim_start_matches("db0:");
    let mut keys = 0u64;
    let mut expires = 0u64;
    for part in payload.split(',') {
        if let Some((k, v)) = part.split_once('=') {
            match k {
                "keys" => keys = v.parse().unwrap(),
                "expires" => expires = v.parse().unwrap(),
                _ => {}
            }
        }
    }
    (keys, expires)
}

// ---------------------------------------------------------------------------
// T049 — INFO format, sections, counters
// ---------------------------------------------------------------------------

#[tokio::test]
async fn info_bare_has_all_documented_sections() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    let text = info(&mut c, &[]).await;
    // One casing is asserted: the `# Section` header capitalizes the first
    // letter of each documented section, in the documented order.
    for sec in ["Server", "Clients", "Memory", "Stats", "Keyspace", "Replication"] {
        let header = format!("# {sec}");
        assert!(text.contains(&header), "missing {header:?} in:\n{text}");
    }
    let order: Vec<usize> = ["# Server", "# Clients", "# Memory", "# Stats", "# Keyspace", "# Replication"]
        .iter()
        .map(|h| text.find(h).expect("section present"))
        .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "sections out of order: {order:?}");

    // CRLF `field:value` lines, per FR-010.
    assert!(text.contains("\r\n"), "INFO must use CRLF line endings");
    assert_eq!(field(&text, "redis_version").as_deref(), Some("7.4.0-spacestorage"));
    assert_eq!(field(&text, "redis_mode").as_deref(), Some("standalone"));
    assert!(field(&text, "uptime_in_seconds").is_some());
    assert!(field(&text, "connected_clients").is_some());
    assert!(field(&text, "role").is_some());
    // No fabricated stock template fields with no internal equivalent.
    assert!(!text.contains("mem_fragmentation_ratio"));
    assert!(!text.contains("aof_enabled"));
    assert!(!text.contains("rdb_"));

    lab.stop();
}

#[tokio::test]
async fn info_section_argument_selects_one_section() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    let server = info(&mut c, &["server"]).await;
    assert!(server.contains("# Server"));
    assert_eq!(field(&server, "redis_version").as_deref(), Some("7.4.0-spacestorage"));
    assert!(!server.contains("# Clients"), "only the requested section: {server}");

    // Section names are case-insensitive.
    let repl = info(&mut c, &["REPLICATION"]).await;
    assert!(repl.contains("# Replication"));
    assert!(field(&repl, "role").is_some());

    // `all`/`everything` additionally include `commandstats`.
    let all = info(&mut c, &["all"]).await;
    assert!(all.contains("# Commandstats"), "INFO all: {all}");
    assert!(all.contains("# Server"));

    // An unknown section is an empty bulk string.
    let unknown = info(&mut c, &["definitely-not-a-section"]).await;
    assert!(unknown.is_empty(), "unknown section -> empty: {unknown:?}");

    lab.stop();
}

#[tokio::test]
async fn info_keyspace_and_dbsize_match_live_keys() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    assert_eq!(bulk(&q(&mut c, &["SET", "a", "1"]).await), "OK");
    assert_eq!(bulk(&q(&mut c, &["SET", "b", "2"]).await), "OK");
    assert_eq!(bulk(&q(&mut c, &["SET", "c", "3"]).await), "OK");
    // One key carries a TTL and counts toward `expires`.
    assert_eq!(bulk(&q(&mut c, &["SETEX", "withttl", "60", "x"]).await), "OK");

    assert_eq!(int(&q(&mut c, &["DBSIZE"]).await), 4, "DBSIZE counts live keys");
    let ks = info(&mut c, &["keyspace"]).await;
    let (keys, expires) = keyspace_counts(&ks);
    assert_eq!(keys, 4, "{ks}");
    assert_eq!(expires, 1, "only `withttl` carries a TTL: {ks}");

    // An already-expired key is not counted (purged on read).
    assert_eq!(bulk(&q(&mut c, &["SET", "gone", "v", "PX", "1"]).await), "OK");
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert_eq!(int(&q(&mut c, &["DBSIZE"]).await), 4, "expired key not counted");
    let ks = info(&mut c, &["keyspace"]).await;
    assert_eq!(keyspace_counts(&ks).0, 4, "expired key not counted: {ks}");

    lab.stop();
}

#[tokio::test]
async fn info_stats_counters_move_with_real_ops() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    let before = stats_snapshot(&info(&mut c, &["stats"]).await);

    // Known writes + reads (SC-006): 3 SET, 2 hits, 1 miss.
    assert_eq!(bulk(&q(&mut c, &["SET", "k1", "v1"]).await), "OK");
    assert_eq!(bulk(&q(&mut c, &["SET", "k2", "v2"]).await), "OK");
    assert_eq!(bulk(&q(&mut c, &["SET", "k3", "v3"]).await), "OK");
    assert_eq!(bulk(&q(&mut c, &["GET", "k1"]).await), "v1");
    assert_eq!(bulk(&q(&mut c, &["GET", "k2"]).await), "v2");
    assert!(matches!(q(&mut c, &["GET", "absent"]).await, Value::Nil));

    let after = stats_snapshot(&info(&mut c, &["stats"]).await);

    let hits = after["keyspace_hits"] - before["keyspace_hits"];
    let misses = after["keyspace_misses"] - before["keyspace_misses"];
    assert_eq!(hits, 2, "two GET hits");
    assert_eq!(misses, 1, "one GET miss");

    // 6 data commands + the first `INFO stats` itself = 7; the second `INFO` is
    // not yet counted when it reads its own snapshot.
    let cmds = after["total_commands_processed"] - before["total_commands_processed"];
    assert_eq!(cmds, 7, "commands processed delta");

    // Expired-key purges are counted.
    let expired_before = after["expired_keys"];
    assert_eq!(bulk(&q(&mut c, &["SET", "ek", "v", "PX", "1"]).await), "OK");
    tokio::time::sleep(Duration::from_millis(40)).await;
    let _ = q(&mut c, &["GET", "ek"]).await;
    let after2 = stats_snapshot(&info(&mut c, &["stats"]).await);
    assert!(
        after2["expired_keys"] > expired_before,
        "expired_keys must increase after a purge-on-read"
    );

    lab.stop();
}

#[tokio::test]
async fn info_type_and_object_subset() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    assert_eq!(bulk(&q(&mut c, &["SET", "s", "v"]).await), "OK");
    assert_eq!(int(&q(&mut c, &["HSET", "h", "f", "v"]).await), 1);
    assert_eq!(int(&q(&mut c, &["LPUSH", "l", "a"]).await), 1);
    assert_eq!(int(&q(&mut c, &["SADD", "st", "a"]).await), 1);
    assert_eq!(int(&q(&mut c, &["ZADD", "z", "1", "m"]).await), 1);
    assert!(matches!(q(&mut c, &["XADD", "x", "*", "f", "v"]).await, Value::BulkString(_)));

    // Redis-visible type matches the creating command.
    assert_eq!(bulk(&q(&mut c, &["TYPE", "s"]).await), "string");
    assert_eq!(bulk(&q(&mut c, &["TYPE", "h"]).await), "hash");
    assert_eq!(bulk(&q(&mut c, &["TYPE", "l"]).await), "list");
    assert_eq!(bulk(&q(&mut c, &["TYPE", "st"]).await), "set");
    assert_eq!(bulk(&q(&mut c, &["TYPE", "z"]).await), "zset");
    assert_eq!(bulk(&q(&mut c, &["TYPE", "x"]).await), "stream");
    assert_eq!(bulk(&q(&mut c, &["TYPE", "missing"]).await), "none");

    // OBJECT subset: ENCODING + IDLETIME succeed.
    assert_eq!(bulk(&q(&mut c, &["OBJECT", "ENCODING", "s"]).await), "raw");
    assert_eq!(bulk(&q(&mut c, &["OBJECT", "ENCODING", "h"]).await), "hashtable");
    assert_eq!(int(&q(&mut c, &["OBJECT", "IDLETIME", "s"]).await), 0);
    // Another OBJECT subcommand is not-supported (an error, never a success).
    let e = qerr(&mut c, &["OBJECT", "FREQ", "s"]).await;
    assert!(e.contains("unknown"), "OBJECT FREQ must error, got {e:?}");

    lab.stop();
}

#[tokio::test]
async fn info_connection_meta_commands() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    // TIME -> [unix_seconds, microseconds].
    let t: Vec<String> = redis::cmd("TIME")
        .query_async(&mut c)
        .await
        .expect("TIME");
    assert_eq!(t.len(), 2, "TIME two parts: {t:?}");
    assert!(t[0].parse::<i64>().is_ok(), "seconds integer: {t:?}");
    assert!(t[1].parse::<i64>().is_ok(), "micros integer: {t:?}");

    // ECHO echoes its argument.
    assert_eq!(bulk(&q(&mut c, &["ECHO", "hello"]).await), "hello");

    // SELECT 0 is a no-op; data verbs still work afterwards.
    assert_eq!(bulk(&q(&mut c, &["SELECT", "0"]).await), "OK");
    assert_eq!(bulk(&q(&mut c, &["SET", "after-select", "v"]).await), "OK");
    assert_eq!(bulk(&q(&mut c, &["GET", "after-select"]).await), "v");

    lab.stop();
}

#[tokio::test]
async fn info_command_table_listing() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    let expected = REDIS_CAPABILITY_TABLE.len() as i64;
    assert_eq!(int(&q(&mut c, &["COMMAND", "COUNT"]).await), expected);

    // Bare `COMMAND` lists every capability-table row.
    let all = q(&mut c, &["COMMAND"]).await;
    assert_eq!(arr(&all).len() as i64, expected, "COMMAND lists the whole table");

    // `COMMAND INFO <name>` returns an info array per requested name.
    let one = q(&mut c, &["COMMAND", "INFO", "GET"]).await;
    let items = arr(&one);
    assert_eq!(items.len(), 1, "one requested name: {one:?}");
    let row = arr(&items[0]);
    assert_eq!(bulk(&row[0]), "get", "first element is the command name");
    // [name, arity, flags, first_key, last_key, step].
    assert!(matches!(row[1], Value::Int(_)), "arity present: {row:?}");
    assert!(matches!(row[2], Value::Array(_)), "flags array: {row:?}");

    lab.stop();
}

#[tokio::test]
async fn info_config_and_client_shapes() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    // CONFIG SET/GET max_block round-trips.
    assert_eq!(bulk(&q(&mut c, &["CONFIG", "SET", "max_block", "5"]).await), "OK");
    let got: Vec<String> = redis::cmd("CONFIG")
        .arg("GET")
        .arg("max_block")
        .query_async(&mut c)
        .await
        .expect("CONFIG GET");
    assert_eq!(got, vec!["max_block".to_string(), "5".to_string()]);
    // An unknown CONFIG parameter is omitted on GET and an error on SET.
    let empty: Vec<String> = redis::cmd("CONFIG")
        .arg("GET")
        .arg("not-a-param")
        .query_async(&mut c)
        .await
        .expect("CONFIG GET");
    assert!(empty.is_empty(), "unknown param omitted: {empty:?}");
    let e = qerr(&mut c, &["CONFIG", "SET", "not-a-param", "x"]).await;
    assert!(e.contains("Unsupported") || e.contains("unknown"), "{e}");

    // CLIENT subset.
    assert!(int(&q(&mut c, &["CLIENT", "ID"]).await) > 0);
    assert_eq!(bulk(&q(&mut c, &["CLIENT", "SETNAME", "conn-1"]).await), "OK");
    assert_eq!(bulk(&q(&mut c, &["CLIENT", "GETNAME"]).await), "conn-1");
    let list = bulk(&q(&mut c, &["CLIENT", "LIST"]).await);
    assert!(list.contains("id="), "CLIENT LIST line: {list:?}");
    assert!(list.contains("db=0"), "CLIENT LIST line: {list:?}");

    lab.stop();
}

#[tokio::test]
async fn info_quit_replies_ok_then_closes() {
    let lab: RedisLab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    let ok: String = redis::cmd("QUIT").query_async(&mut c).await.expect("QUIT");
    assert_eq!(ok, "OK");

    // The server closed the connection: a following command cannot succeed.
    let after = redis::cmd("PING").query_async::<Value>(&mut c).await;
    assert!(after.is_err(), "connection must be closed after QUIT: {after:?}");

    lab.stop();
}

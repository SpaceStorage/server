//! `017` T050: in-process `TYPE`/`OBJECT` unit coverage plus the `INFO`/`CONFIG`
//! shapes and not-supported forms (`SWAPDB`, a bad `OBJECT` subcommand).
//!
//! These drive `dispatch` directly over a `SessionState` so the introspection
//! family is checked at the handler seam without a socket.

use std::sync::{Arc, RwLock};

use spacestorage_compat::{DialectProfile, REDIS_CAPABILITY_TABLE};
use spacestorage_handler_redis::{
    BitmapValue, EntryPayload, HllValue, RedisReply, SessionState, dispatch,
};
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

fn text(r: RedisReply) -> String {
    String::from_utf8(bulk(r)).expect("utf8")
}

fn simple(r: RedisReply) -> String {
    match r {
        RedisReply::Simple(s) => s,
        other => panic!("expected simple string, got {other:?}"),
    }
}

fn int(r: RedisReply) -> i64 {
    match r {
        RedisReply::Integer(i) => i,
        other => panic!("expected integer, got {other:?}"),
    }
}

fn err(r: RedisReply) -> String {
    match r {
        RedisReply::Error(e) => e,
        other => panic!("expected error, got {other:?}"),
    }
}

#[test]
fn type_reports_redis_visible_name() {
    let mut s = complete_session();
    assert!(matches!(dispatch(&mut s, "SET", &["s", "v"]), RedisReply::Ok));
    assert!(matches!(dispatch(&mut s, "HSET", &["h", "f", "v"]), RedisReply::Integer(_)));
    assert!(matches!(dispatch(&mut s, "LPUSH", &["l", "a"]), RedisReply::Integer(_)));
    assert!(matches!(dispatch(&mut s, "SADD", &["st", "a"]), RedisReply::Integer(_)));
    assert!(matches!(dispatch(&mut s, "ZADD", &["z", "1", "m"]), RedisReply::Integer(_)));
    assert!(matches!(dispatch(&mut s, "XADD", &["x", "*", "f", "v"]), RedisReply::Bulk(Some(_))));

    assert_eq!(simple(dispatch(&mut s, "TYPE", &["s"])), "string");
    assert_eq!(simple(dispatch(&mut s, "TYPE", &["h"])), "hash");
    assert_eq!(simple(dispatch(&mut s, "TYPE", &["l"])), "list");
    assert_eq!(simple(dispatch(&mut s, "TYPE", &["st"])), "set");
    assert_eq!(simple(dispatch(&mut s, "TYPE", &["z"])), "zset");
    assert_eq!(simple(dispatch(&mut s, "TYPE", &["x"])), "stream");
    // A missing key is `none`, never an error.
    assert_eq!(simple(dispatch(&mut s, "TYPE", &["absent"])), "none");
}

#[test]
fn bitmap_and_hll_visible_as_string() {
    // The Redis-visible mapping is a pure function of the payload (FR-004/FR-017).
    use spacestorage_handler_redis::RedisVisibleType;
    assert_eq!(
        EntryPayload::Bitmap(BitmapValue(vec![1])).visible(),
        RedisVisibleType::String
    );
    assert_eq!(
        EntryPayload::HyperLogLog(HllValue {
            precision: 14,
            registers: vec![0; 16],
        })
        .visible(),
        RedisVisibleType::String
    );
    assert_eq!(RedisVisibleType::String.as_str(), "string");
}

#[test]
fn object_encoding_and_idletime_subset() {
    let mut s = complete_session();
    assert!(matches!(dispatch(&mut s, "SET", &["s", "v"]), RedisReply::Ok));
    assert!(matches!(dispatch(&mut s, "HSET", &["h", "f", "v"]), RedisReply::Integer(_)));

    assert_eq!(text(dispatch(&mut s, "OBJECT", &["ENCODING", "s"])), "raw");
    assert_eq!(text(dispatch(&mut s, "OBJECT", &["ENCODING", "h"])), "hashtable");
    // No persisted access timestamp: report 0, never a fabricated idle time.
    assert_eq!(int(dispatch(&mut s, "OBJECT", &["IDLETIME", "s"])), 0);
    // A missing key is an error for OBJECT (stock Redis: no such key).
    assert!(err(dispatch(&mut s, "OBJECT", &["ENCODING", "absent"])).contains("no such key"));
}

#[test]
fn object_unknown_subcommand_and_swapdb_error() {
    let mut s = complete_session();
    assert!(matches!(dispatch(&mut s, "SET", &["s", "v"]), RedisReply::Ok));

    // Any `OBJECT` subcommand outside the documented subset is refused.
    let e = err(dispatch(&mut s, "OBJECT", &["FREQ", "s"]));
    assert!(e.contains("unknown"), "OBJECT FREQ: {e:?}");
    let e = err(dispatch(&mut s, "OBJECT", &["ENCODING"]));
    assert!(e.contains("unknown subcommand"), "OBJECT ENCODING arity: {e:?}");

    // `SWAPDB` is not-supported (namespaces replace numbered databases).
    let e = err(dispatch(&mut s, "SWAPDB", &["0", "1"]));
    assert!(e.contains("unknown command 'SWAPDB'"), "{e:?}");
}

#[test]
fn info_sections_present() {
    let mut s = complete_session();
    let body = text(dispatch(&mut s, "INFO", &[]));
    for sec in ["# Server", "# Clients", "# Memory", "# Stats", "# Keyspace", "# Replication"] {
        assert!(body.contains(sec), "missing {sec:?} in:\n{body}");
    }
    assert!(body.contains("redis_version:7.4.0-spacestorage"));
    assert!(body.contains("\r\n"), "CRLF format");
    // Fields with no internal equivalent are omitted, never faked.
    assert!(!body.contains("mem_fragmentation_ratio"));
    assert!(!body.contains("aof_enabled"));

    // A single section is selectable; an unknown one is empty.
    let server = text(dispatch(&mut s, "INFO", &["server"]));
    assert!(server.contains("# Server") && !server.contains("# Clients"));
    let unknown = text(dispatch(&mut s, "INFO", &["nope"]));
    assert!(unknown.is_empty());
}

#[test]
fn dbsize_counts_live_keys() {
    let mut s = complete_session();
    assert_eq!(int(dispatch(&mut s, "DBSIZE", &[])), 0);
    assert!(matches!(dispatch(&mut s, "SET", &["a", "1"]), RedisReply::Ok));
    assert!(matches!(dispatch(&mut s, "SET", &["b", "2"]), RedisReply::Ok));
    assert_eq!(int(dispatch(&mut s, "DBSIZE", &[])), 2);
}

#[test]
fn config_get_set_shape() {
    let mut s = complete_session();
    // Unset: `CONFIG GET max_block` is an empty array.
    match dispatch(&mut s, "CONFIG", &["GET", "max_block"]) {
        RedisReply::Array(v) => assert!(v.is_empty(), "{v:?}"),
        other => panic!("expected empty array, got {other:?}"),
    }
    assert!(matches!(dispatch(&mut s, "CONFIG", &["SET", "max_block", "5"]), RedisReply::Ok));
    match dispatch(&mut s, "CONFIG", &["GET", "max_block"]) {
        RedisReply::Array(v) => {
            assert_eq!(v.len(), 2);
            assert_eq!(text(v[0].clone()), "max_block");
            assert_eq!(text(v[1].clone()), "5");
        }
        other => panic!("expected array, got {other:?}"),
    }
    // A non-numeric value and an unknown parameter both error on SET.
    assert!(err(dispatch(&mut s, "CONFIG", &["SET", "max_block", "abc"])).contains("integer"));
    assert!(err(dispatch(&mut s, "CONFIG", &["SET", "other", "x"])).contains("Unsupported"));
    // An unknown parameter is omitted on GET.
    match dispatch(&mut s, "CONFIG", &["GET", "other"]) {
        RedisReply::Array(v) => assert!(v.is_empty()),
        other => panic!("expected empty array, got {other:?}"),
    }
}

#[test]
fn command_count_matches_table() {
    let mut s = complete_session();
    assert_eq!(
        int(dispatch(&mut s, "COMMAND", &["COUNT"])),
        REDIS_CAPABILITY_TABLE.len() as i64
    );
    match dispatch(&mut s, "COMMAND", &[]) {
        RedisReply::Array(rows) => assert_eq!(rows.len(), REDIS_CAPABILITY_TABLE.len()),
        other => panic!("expected array, got {other:?}"),
    }
    match dispatch(&mut s, "COMMAND", &["INFO", "GET"]) {
        RedisReply::Array(rows) => {
            assert_eq!(rows.len(), 1);
            match &rows[0] {
                RedisReply::Array(info) => assert_eq!(text(info[0].clone()), "get"),
                other => panic!("expected info array, got {other:?}"),
            }
        }
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn time_echo_and_client_shapes() {
    let mut s = complete_session();
    match dispatch(&mut s, "TIME", &[]) {
        RedisReply::Array(v) => {
            assert_eq!(v.len(), 2);
            assert!(text(v[0].clone()).parse::<i64>().is_ok());
            assert!(text(v[1].clone()).parse::<i64>().is_ok());
        }
        other => panic!("expected array, got {other:?}"),
    }
    assert_eq!(text(dispatch(&mut s, "ECHO", &["hi"])), "hi");
    // ECHO arity errors.
    assert!(err(dispatch(&mut s, "ECHO", &[])).contains("wrong number of arguments"));

    // CLIENT ID / SETNAME / GETNAME.
    assert!(int(dispatch(&mut s, "CLIENT", &["ID"])) >= 0);
    assert!(matches!(dispatch(&mut s, "CLIENT", &["SETNAME", "n1"]), RedisReply::Ok));
    assert_eq!(text(dispatch(&mut s, "CLIENT", &["GETNAME"])), "n1");
}

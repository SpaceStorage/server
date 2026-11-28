//! `017` T064: limits conformance (SC-008) against an unmodified stock Redis
//! client.
//!
//! Exceeding a documented key/value size limit, an out-of-range bitmap offset,
//! an absurd offset, or a stream-ID boundary overflow must be rejected with the
//! limit named in Redis form, in 100% of cases, with **0 hangs**. Every live case
//! runs under a hard deadline so a hang fails loudly instead of blocking.
#![cfg(feature = "redis-dialect")]

mod support;

use std::sync::Arc;
use std::time::Duration;

use redis::Value;
use spacestorage_compat::{
    LimitServices, Limits, SessionCounters, DEFAULT_MAX_KEY, DEFAULT_MAX_VALUE,
};
use support::redis_live::boot_redis_complete_product;

type Conn = redis::aio::MultiplexedConnection;

/// Run `parts` expecting an error within 5s, returning its rendered message.
async fn qerr(c: &mut Conn, parts: &[&str]) -> String {
    let mut cmd = redis::cmd(parts[0]);
    for p in &parts[1..] {
        cmd.arg(*p);
    }
    match tokio::time::timeout(Duration::from_secs(5), cmd.query_async::<Value>(c)).await {
        Ok(Ok(v)) => panic!("{parts:?} must be rejected, got {v:?}"),
        Ok(Err(e)) => e.to_string(),
        Err(_) => panic!("{parts:?} hung past 5s"),
    }
}

/// A `SET` whose value exceeds `limits.max_value` (16 MiB by default) is rejected
/// naming the `value` limit, before any state is written.
#[tokio::test]
async fn value_limit_rejects_oversized_set() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    let big = "x".repeat(DEFAULT_MAX_VALUE as usize + 1);
    let e = qerr(&mut c, &["SET", "big", big.as_str()]).await;
    assert!(e.contains("limit_exceeded"), "{e}");
    assert!(e.contains("limit:value"), "limit named: {e}");
    // Rejected before the op: the key was never created.
    let exists: i64 = redis::cmd("EXISTS")
        .arg("big")
        .query_async(&mut c)
        .await
        .expect("EXISTS");
    assert_eq!(exists, 0, "no partial write on a rejected value");

    lab.stop();
}

/// A key longer than `limits.max_key` (1 KiB by default) is rejected naming the
/// `key` limit.
#[tokio::test]
async fn key_limit_rejects_oversized_key() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    let key = "k".repeat(DEFAULT_MAX_KEY as usize + 1);
    let e = qerr(&mut c, &["SET", key.as_str(), "v"]).await;
    assert!(e.contains("limit_exceeded"), "{e}");
    assert!(e.contains("limit:key"), "limit named: {e}");

    lab.stop();
}

/// `SETRANGE` that would grow the value past `limits.max_value` is rejected
/// naming the `value` limit — never a silent oversize write.
#[tokio::test]
async fn setrange_growth_past_limit_rejects() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    // offset + patch length = max_value + 1.
    let off = (DEFAULT_MAX_VALUE - 1).to_string();
    let e = qerr(&mut c, &["SETRANGE", "sr", off.as_str(), "ab"]).await;
    assert!(e.contains("limit_exceeded"), "{e}");
    assert!(e.contains("limit:value"), "limit named: {e}");
    let exists: i64 = redis::cmd("EXISTS")
        .arg("sr")
        .query_async(&mut c)
        .await
        .expect("EXISTS");
    assert_eq!(exists, 0, "no partial write on a rejected SETRANGE");

    lab.stop();
}

/// A bitmap offset whose resulting byte length exceeds `limits.max_value` is
/// rejected naming the `value` limit (the last legal offset still stores).
#[tokio::test]
async fn bitmap_offset_past_value_limit_rejects() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    let over = (DEFAULT_MAX_VALUE * 8).to_string();
    let e = qerr(&mut c, &["SETBIT", "bm", over.as_str(), "1"]).await;
    assert!(e.contains("limit_exceeded"), "{e}");
    assert!(e.contains("limit:value"), "limit named: {e}");

    let at_max = (DEFAULT_MAX_VALUE * 8 - 1).to_string();
    let prev: i64 = redis::cmd("SETBIT")
        .arg("atmax")
        .arg(at_max.as_str())
        .arg("1")
        .query_async(&mut c)
        .await
        .expect("SETBIT at the last legal offset");
    assert_eq!(prev, 0);

    lab.stop();
}

/// A bit offset at or above `2^32` is out of range for both `SETBIT` and
/// `GETBIT`: the documented `bit offset` error, never a silent success.
#[tokio::test]
async fn bitmap_offset_at_or_above_2pow32_rejects() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    for off in ["4294967296", "18446744073709551615"] {
        let e = qerr(&mut c, &["SETBIT", "bm", off, "1"]).await;
        assert!(e.contains("bit offset"), "SETBIT {off} -> {e}");
        let e = qerr(&mut c, &["GETBIT", "bm", off]).await;
        assert!(e.contains("bit offset"), "GETBIT {off} -> {e}");
    }

    lab.stop();
}

/// A stream whose top ID is the `u64::MAX-u64::MAX` boundary rejects a further
/// `XADD` with the documented "equal or smaller" error, never an overflow.
#[tokio::test]
async fn stream_id_boundary_overflow_rejects() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    let id: String = redis::cmd("XADD")
        .arg("s")
        .arg("18446744073709551615-18446744073709551615")
        .arg("f")
        .arg("v")
        .query_async(&mut c)
        .await
        .expect("XADD at the ID ceiling");
    assert!(!id.is_empty());

    // Auto-sequenced append cannot advance past the ceiling.
    let e = qerr(&mut c, &["XADD", "s", "*", "f", "v"]).await;
    assert!(e.contains("equal or smaller"), "{e}");
    // An explicit smaller id is likewise refused.
    let e = qerr(&mut c, &["XADD", "s", "1-1", "f", "v"]).await;
    assert!(e.contains("equal or smaller"), "{e}");

    lab.stop();
}

/// Unit-level guard: a fixed `LimitServices` with a tiny `max_value` reports the
/// named `limit:value` rejection, proving the conformance cases above exercise
/// the configured limits rather than a hard-coded constant.
#[test]
fn fixed_limits_reject_oversize_value() {
    let mut lim = Limits::default();
    lim.max_value = 4;
    let svc = LimitServices::with_fixed(Arc::new(SessionCounters::new()), lim);
    let effective = svc.limits();
    assert!(effective.check_value(4).is_ok(), "4 bytes == max is legal");
    let err = effective.check_value(5).expect_err("5 bytes > max");
    let msg = err.to_string();
    assert!(msg.contains("limit_exceeded"), "{msg}");
    assert!(msg.contains("limit:value"), "{msg}");
}

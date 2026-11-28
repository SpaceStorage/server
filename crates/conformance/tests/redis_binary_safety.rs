//! `017` T029: byte-safety regression for the shared entry channel.
//!
//! T029 routes every key-space entry read/write through the shared IR
//! (`LogicalRequest::KeyGet` / `Object` / `KeyDelete` / `KeyScan` →
//! `PlannerEngine`) instead of a direct `ContainerCatalog` mutation. The stored
//! value is the canonical `lower::encode_entry` blob, which is binary (LE `u64`
//! version/expire header, an LE `u32` length prefix, and LE `f64` zset scores).
//! A `String`-coercing read path (`from_utf8_lossy`, as `object_get` uses)
//! would corrupt it, so these tests pin the byte-exact round-trip.
//!
//! Not covered here: non-UTF-8 *wire arguments*. The RESP command surface takes
//! `&[&str]` (`handler-redis::handler::frame_to_string` does `String::from_utf8`
//! on every bulk arg), so an argument that is not valid UTF-8 is refused before
//! it reaches the IR. That is a pre-existing boundary of this handler and is not
//! required by the `017` spec; T029 owns only the store channel below.
#![cfg(feature = "redis-dialect")]

mod support;

use support::redis_live::boot_redis_complete_product;

/// A binary zset score (LE `f64` bytes with high bits set) must survive the
/// shared store channel: a `from_utf8_lossy` read would break `ZSCORE`.
#[tokio::test]
async fn binary_score_blob_round_trips() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    let n: i64 = redis::cmd("ZADD")
        .arg("z")
        .arg("1.5")
        .arg("m")
        .query_async(&mut c)
        .await
        .unwrap();
    assert_eq!(n, 1);
    let score: String = redis::cmd("ZSCORE")
        .arg("z")
        .arg("m")
        .query_async(&mut c)
        .await
        .unwrap();
    assert_eq!(score, "1.5");

    lab.stop();
}

/// A value longer than 127 bytes puts a `>= 0x80` byte into the entry blob's
/// LE `u32` length prefix, which a lossy UTF-8 read would corrupt. The exact
/// bytes must round-trip.
#[tokio::test]
async fn long_value_header_round_trips() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    let payload: String = "abcdefghij".repeat(30); // 300 bytes → length byte >= 0x80
    let ok: String = redis::cmd("SET")
        .arg("k")
        .arg(payload.as_str())
        .query_async(&mut c)
        .await
        .unwrap();
    assert_eq!(ok, "OK");

    let got: Vec<u8> = redis::cmd("GET")
        .arg("k")
        .query_async(&mut c)
        .await
        .unwrap();
    assert_eq!(got, payload.as_bytes(), "long value must round-trip byte-exact");

    lab.stop();
}

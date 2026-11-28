//! US5 (`017` T056–T057): complete-product bitmap + HyperLogLog conformance
//! against an unmodified stock Redis client. Every test name carries `bitmap`
//! or `hll` so `cargo test -- bitmap` / `-- hll` selects this suite.
#![cfg(feature = "redis-dialect")]

mod support;

use support::redis_live::boot_redis_complete_product;

type Conn = redis::aio::MultiplexedConnection;

/// Run `parts` (command + string args) on a stock client.
async fn q<T: redis::FromRedisValue>(c: &mut Conn, parts: &[&str]) -> redis::RedisResult<T> {
    let mut cmd = redis::cmd(parts[0]);
    for p in &parts[1..] {
        cmd.arg(*p);
    }
    cmd.query_async(c).await
}

/// Run `parts` expecting an error, returning its rendered message.
async fn qerr(c: &mut Conn, parts: &[&str]) -> String {
    match q::<i64>(c, parts).await {
        Err(e) => format!("{e}"),
        Ok(v) => panic!("expected an error for {parts:?}, got {v}"),
    }
}

/// Build `bytes` at `key` using only `SETBIT`, so every wire argument stays
/// valid UTF-8. The RESP command surface rejects non-UTF-8 bulk args
/// (`handler-redis::handler::frame_to_string`) — a documented handler boundary
/// (see `redis_binary_safety.rs`). Writing the final bit (even `0`) extends the
/// value to `bytes.len()` bytes, exactly like Redis `SETBIT`.
async fn set_bin(c: &mut Conn, key: &str, bytes: &[u8]) {
    assert!(!bytes.is_empty());
    let last = (bytes.len() * 8 - 1).to_string();
    q::<i64>(c, &["SETBIT", key, &last, "0"]).await.unwrap();
    for (i, b) in bytes.iter().enumerate() {
        for bit in 0..8u32 {
            if b & (0x80 >> bit) != 0 {
                let off = (i * 8 + bit as usize).to_string();
                q::<i64>(c, &["SETBIT", key, &off, "1"]).await.unwrap();
            }
        }
    }
}

async fn pfadd_range(c: &mut Conn, key: &str, start: u32, end: u32) {
    for chunk in (start..end).collect::<Vec<_>>().chunks(500) {
        let mut cmd = redis::cmd("PFADD");
        cmd.arg(key);
        for i in chunk {
            cmd.arg(format!("u{i}"));
        }
        let _: i64 = cmd.query_async(c).await.expect("PFADD chunk");
    }
}

fn hll_bound(n: f64) -> f64 {
    // 1.04 / sqrt(2^14) ≈ 0.81%, the registered `hyperloglog` standard error.
    n * (1.04 / (2f64.powi(14)).sqrt())
}

// ---- T056: bitmaps ---------------------------------------------------------

#[tokio::test]
async fn bitmap_setbit_getbit_type() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    assert_eq!(q::<i64>(&mut c, &["SETBIT", "bm", "0", "1"]).await.unwrap(), 0);
    assert_eq!(q::<i64>(&mut c, &["SETBIT", "bm", "7", "1"]).await.unwrap(), 0);
    assert_eq!(q::<i64>(&mut c, &["SETBIT", "bm", "15", "1"]).await.unwrap(), 0);
    // Re-setting an already-set bit returns the previous value.
    assert_eq!(q::<i64>(&mut c, &["SETBIT", "bm", "0", "1"]).await.unwrap(), 1);
    assert_eq!(q::<i64>(&mut c, &["SETBIT", "bm", "1", "0"]).await.unwrap(), 0);

    assert_eq!(q::<i64>(&mut c, &["GETBIT", "bm", "0"]).await.unwrap(), 1);
    assert_eq!(q::<i64>(&mut c, &["GETBIT", "bm", "1"]).await.unwrap(), 0);
    assert_eq!(q::<i64>(&mut c, &["GETBIT", "bm", "7"]).await.unwrap(), 1);
    assert_eq!(q::<i64>(&mut c, &["GETBIT", "bm", "15"]).await.unwrap(), 1);
    // Missing bit (beyond the value) defaults to 0.
    assert_eq!(q::<i64>(&mut c, &["GETBIT", "bm", "1000"]).await.unwrap(), 0);
    assert_eq!(q::<i64>(&mut c, &["GETBIT", "absent", "3"]).await.unwrap(), 0);

    let ty: String = q(&mut c, &["TYPE", "bm"]).await.unwrap();
    assert_eq!(ty, "string");
    lab.stop();
}

#[tokio::test]
async fn bitmap_bitcount_byte_and_bit_ranges() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;
    // bm = [0b10000001, 0b00000001]
    q::<i64>(&mut c, &["SETBIT", "bm", "0", "1"]).await.unwrap();
    q::<i64>(&mut c, &["SETBIT", "bm", "7", "1"]).await.unwrap();
    q::<i64>(&mut c, &["SETBIT", "bm", "15", "1"]).await.unwrap();

    assert_eq!(q::<i64>(&mut c, &["BITCOUNT", "bm"]).await.unwrap(), 3);
    assert_eq!(q::<i64>(&mut c, &["BITCOUNT", "bm", "0", "0"]).await.unwrap(), 2);
    assert_eq!(q::<i64>(&mut c, &["BITCOUNT", "bm", "1", "1"]).await.unwrap(), 1);
    assert_eq!(q::<i64>(&mut c, &["BITCOUNT", "bm", "-1", "-1"]).await.unwrap(), 1);
    assert_eq!(q::<i64>(&mut c, &["BITCOUNT", "bm", "1", "-1"]).await.unwrap(), 1);
    // Empty/invalid range -> 0.
    assert_eq!(q::<i64>(&mut c, &["BITCOUNT", "bm", "5", "2"]).await.unwrap(), 0);
    // BIT units.
    assert_eq!(q::<i64>(&mut c, &["BITCOUNT", "bm", "0", "0", "BIT"]).await.unwrap(), 1);
    assert_eq!(q::<i64>(&mut c, &["BITCOUNT", "bm", "0", "7", "BIT"]).await.unwrap(), 2);
    assert_eq!(q::<i64>(&mut c, &["BITCOUNT", "bm", "0", "0", "BYTE"]).await.unwrap(), 2);
    // Missing key -> 0.
    assert_eq!(q::<i64>(&mut c, &["BITCOUNT", "absent"]).await.unwrap(), 0);
    lab.stop();
}

#[tokio::test]
async fn bitmap_bitpos_ranges_and_clear_scan() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;
    // bm = [0b10000001, 0b00000001]
    q::<i64>(&mut c, &["SETBIT", "bm", "0", "1"]).await.unwrap();
    q::<i64>(&mut c, &["SETBIT", "bm", "7", "1"]).await.unwrap();
    q::<i64>(&mut c, &["SETBIT", "bm", "15", "1"]).await.unwrap();

    assert_eq!(q::<i64>(&mut c, &["BITPOS", "bm", "1"]).await.unwrap(), 0);
    assert_eq!(q::<i64>(&mut c, &["BITPOS", "bm", "1", "1"]).await.unwrap(), 15);
    assert_eq!(q::<i64>(&mut c, &["BITPOS", "bm", "1", "2"]).await.unwrap(), -1);
    assert_eq!(q::<i64>(&mut c, &["BITPOS", "bm", "0"]).await.unwrap(), 1);
    // Missing key: no 1 bit, first 0 bit is at position 0.
    assert_eq!(q::<i64>(&mut c, &["BITPOS", "absent", "1"]).await.unwrap(), -1);
    assert_eq!(q::<i64>(&mut c, &["BITPOS", "absent", "0"]).await.unwrap(), 0);

    // All-ones value: a clear-bit scan without an explicit end reports the
    // total bit length; with an explicit end it is -1.
    for off in 0..16 {
        q::<i64>(&mut c, &["SETBIT", "ones", &off.to_string(), "1"])
            .await
            .unwrap();
    }
    assert_eq!(q::<i64>(&mut c, &["BITPOS", "ones", "0"]).await.unwrap(), 16);
    assert_eq!(q::<i64>(&mut c, &["BITPOS", "ones", "0", "0"]).await.unwrap(), 16);
    assert_eq!(q::<i64>(&mut c, &["BITPOS", "ones", "0", "0", "0"]).await.unwrap(), -1);
    assert_eq!(q::<i64>(&mut c, &["BITPOS", "ones", "1"]).await.unwrap(), 0);
    lab.stop();
}

#[tokio::test]
async fn bitmap_bitop_ops_length_rule_and_type() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    set_bin(&mut c, "k1", &[0xF0]).await;
    set_bin(&mut c, "k2", &[0x0F]).await;

    assert_eq!(q::<i64>(&mut c, &["BITOP", "AND", "d", "k1", "k2"]).await.unwrap(), 1);
    assert_eq!(q::<i64>(&mut c, &["BITCOUNT", "d"]).await.unwrap(), 0);
    assert_eq!(q::<i64>(&mut c, &["BITOP", "OR", "d", "k1", "k2"]).await.unwrap(), 1);
    assert_eq!(q::<i64>(&mut c, &["BITCOUNT", "d"]).await.unwrap(), 8);
    let ty: String = q(&mut c, &["TYPE", "d"]).await.unwrap();
    assert_eq!(ty, "string");
    assert_eq!(q::<i64>(&mut c, &["BITOP", "XOR", "d", "k1", "k2"]).await.unwrap(), 1);
    assert_eq!(q::<i64>(&mut c, &["BITCOUNT", "d"]).await.unwrap(), 8);
    assert_eq!(q::<i64>(&mut c, &["BITOP", "NOT", "d", "k1"]).await.unwrap(), 1);
    assert_eq!(q::<i64>(&mut c, &["BITCOUNT", "d"]).await.unwrap(), 4);

    // Length rule: AND/OR/XOR result is the longest source with short operands
    // padded by zero bytes.
    set_bin(&mut c, "l1", &[0xFF, 0x01]).await;
    set_bin(&mut c, "l2", &[0xFF]).await;
    // AND pads `l2` to [0xFF, 0x00] -> [0xFF, 0x00] (8 bits), length 2.
    assert_eq!(q::<i64>(&mut c, &["BITOP", "AND", "d", "l1", "l2"]).await.unwrap(), 2);
    assert_eq!(q::<i64>(&mut c, &["BITCOUNT", "d"]).await.unwrap(), 8);
    assert_eq!(q::<i64>(&mut c, &["BITOP", "OR", "d", "l1", "l2"]).await.unwrap(), 2);
    assert_eq!(q::<i64>(&mut c, &["BITCOUNT", "d"]).await.unwrap(), 9);

    // A missing operand counts as all-zero bytes.
    assert_eq!(q::<i64>(&mut c, &["BITOP", "OR", "d", "missing", "k1"]).await.unwrap(), 1);
    assert_eq!(q::<i64>(&mut c, &["BITCOUNT", "d"]).await.unwrap(), 4);

    // All sources empty/missing: dest is deleted, reply 0.
    let _: String = q(&mut c, &["SET", "d", "x"]).await.unwrap();
    assert_eq!(q::<i64>(&mut c, &["BITOP", "AND", "d", "m1", "m2"]).await.unwrap(), 0);
    assert_eq!(q::<i64>(&mut c, &["EXISTS", "d"]).await.unwrap(), 0);

    // BITOP NOT requires exactly one source.
    let e = qerr(&mut c, &["BITOP", "NOT", "d", "k1", "k2"]).await;
    assert!(e.contains("single source key"), "{e}");
    lab.stop();
}

#[tokio::test]
async fn bitmap_bitfield_get_set_incrby_and_overflow() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    let r: Vec<i64> = q(&mut c, &["BITFIELD", "bf", "SET", "u8", "0", "255"]).await.unwrap();
    assert_eq!(r, vec![255]);
    let g: Vec<i64> = q(&mut c, &["BITFIELD", "bf", "GET", "u8", "0"]).await.unwrap();
    assert_eq!(g, vec![255]);

    // Default WRAP: u8 255 + 1 wraps to 0.
    let w: Vec<i64> = q(&mut c, &["BITFIELD", "bf", "INCRBY", "u8", "0", "1"]).await.unwrap();
    assert_eq!(w, vec![0]);

    // SAT clamps to the type maximum.
    q::<Vec<i64>>(&mut c, &["BITFIELD", "bf", "SET", "u8", "0", "250"]).await.unwrap();
    let sat: Vec<i64> = q(&mut c, &["BITFIELD", "bf", "OVERFLOW", "SAT", "INCRBY", "u8", "0", "10"])
        .await
        .unwrap();
    assert_eq!(sat, vec![255]);

    // FAIL returns nil and leaves the value unchanged.
    let fail: Vec<Option<i64>> =
        q(&mut c, &["BITFIELD", "bf", "OVERFLOW", "FAIL", "INCRBY", "u8", "0", "10"])
            .await
            .unwrap();
    assert_eq!(fail, vec![None]);
    let unchanged: Vec<i64> = q(&mut c, &["BITFIELD", "bf", "GET", "u8", "0"]).await.unwrap();
    assert_eq!(unchanged, vec![255]);

    // Signed fields and signed WRAP.
    let s: Vec<i64> = q(&mut c, &["BITFIELD", "bs", "SET", "i8", "0", "-1"]).await.unwrap();
    assert_eq!(s, vec![-1]);
    let si: Vec<i64> = q(&mut c, &["BITFIELD", "bs", "INCRBY", "i8", "0", "-128"]).await.unwrap();
    assert_eq!(si, vec![127]);
    let sg: Vec<i64> = q(&mut c, &["BITFIELD", "bs", "GET", "i8", "0"]).await.unwrap();
    assert_eq!(sg, vec![127]);

    // `#N` addresses the Nth field of the encoding width.
    let f: Vec<i64> = q(&mut c, &["BITFIELD", "bf", "SET", "u8", "#1", "42"]).await.unwrap();
    assert_eq!(f, vec![42]);
    let fg: Vec<i64> = q(&mut c, &["BITFIELD", "bf", "GET", "u8", "8"]).await.unwrap();
    assert_eq!(fg, vec![42]);

    // BITFIELD_RO accepts only GET.
    let ro: Vec<i64> = q(&mut c, &["BITFIELD_RO", "bf", "GET", "u8", "0"]).await.unwrap();
    assert_eq!(ro, vec![255]);
    let e = qerr(&mut c, &["BITFIELD_RO", "bf", "SET", "u8", "0", "1"]).await;
    assert!(e.contains("BITFIELD_RO"), "{e}");

    // Missing key GET yields 0 without creating the key.
    let z: Vec<i64> = q(&mut c, &["BITFIELD", "absent", "GET", "u8", "0"]).await.unwrap();
    assert_eq!(z, vec![0]);
    assert_eq!(q::<i64>(&mut c, &["EXISTS", "absent"]).await.unwrap(), 0);

    // An absolute offset at or above 2^32 is out of range (Redis caps bit
    // offsets at 2^32 - 1).
    let e = qerr(&mut c, &["BITFIELD", "bf", "GET", "u8", "4294967296"]).await;
    assert!(e.contains("bit offset"), "{e}");
    lab.stop();
}

#[tokio::test]
async fn bitmap_wrongtype_on_non_string_key() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;
    let n: i64 = q(&mut c, &["HSET", "h", "f", "v"]).await.unwrap();
    assert_eq!(n, 1);
    let e = qerr(&mut c, &["SETBIT", "h", "0", "1"]).await;
    assert!(e.contains("WRONGTYPE"), "{e}");
    let e = qerr(&mut c, &["GETBIT", "h", "0"]).await;
    assert!(e.contains("WRONGTYPE"), "{e}");
    lab.stop();
}

#[tokio::test]
async fn bitmap_setbit_oversize_offset_rejects_before_the_op() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    // Default `015` `limits.max_value` is 16 MiB, so the last legal SETBIT
    // offset is `16 MiB * 8 - 1`; one byte past that is a limit rejection.
    let over = (16u64 * 1024 * 1024 * 8).to_string();
    let e = qerr(&mut c, &["SETBIT", "big", &over, "1"]).await;
    assert!(e.contains("limit_exceeded"), "{e}");
    assert!(e.contains("limit:value"), "{e}");
    // Rejected before the op: no partial state, the key is not created.
    assert_eq!(q::<i64>(&mut c, &["EXISTS", "big"]).await.unwrap(), 0);

    // The largest legal offset still stores.
    let at_max = (16u64 * 1024 * 1024 * 8 - 1).to_string();
    assert_eq!(q::<i64>(&mut c, &["SETBIT", "atmax", &at_max, "1"]).await.unwrap(), 0);
    assert_eq!(q::<i64>(&mut c, &["GETBIT", "atmax", &at_max]).await.unwrap(), 1);
    lab.stop();
}

// ---- T057: HyperLogLog -----------------------------------------------------

#[tokio::test]
async fn hll_pfadd_pfcount_within_bound_and_type() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    assert_eq!(q::<i64>(&mut c, &["PFADD", "h", "a", "b", "c"]).await.unwrap(), 1);
    // Re-adding the same elements changes nothing -> 0.
    assert_eq!(q::<i64>(&mut c, &["PFADD", "h", "a", "b", "c"]).await.unwrap(), 0);
    assert_eq!(q::<i64>(&mut c, &["PFADD", "h", "d"]).await.unwrap(), 1);
    let small: i64 = q(&mut c, &["PFCOUNT", "h"]).await.unwrap();
    assert!((3..=5).contains(&small), "small HLL count {small}");

    // A fixed 10 000-element fixture must land within the documented error.
    pfadd_range(&mut c, "big", 0, 10_000).await;
    let est: i64 = q(&mut c, &["PFCOUNT", "big"]).await.unwrap();
    let bound = hll_bound(10_000.0);
    assert!(
        (est as f64 - 10_000.0).abs() <= bound,
        "estimate {est} outside ±{bound:.1} of 10000"
    );

    let ty: String = q(&mut c, &["TYPE", "big"]).await.unwrap();
    assert_eq!(ty, "string");

    // PFADD with no elements on a missing key creates it and reports 1.
    assert_eq!(q::<i64>(&mut c, &["PFADD", "empty"]).await.unwrap(), 1);
    assert_eq!(q::<i64>(&mut c, &["PFCOUNT", "empty"]).await.unwrap(), 0);
    assert_eq!(q::<i64>(&mut c, &["PFCOUNT", "missing-key"]).await.unwrap(), 0);
    lab.stop();
}

#[tokio::test]
async fn hll_pfmerge_behaves_like_sketch_of_union() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    pfadd_range(&mut c, "s1", 0, 5000).await;
    pfadd_range(&mut c, "s2", 2500, 7500).await;
    // Union of 0..5000 and 2500..7500 = 0..7500 (7500 distinct).
    let ok: String = q(&mut c, &["PFMERGE", "dst", "s1", "s2"]).await.unwrap();
    assert_eq!(ok, "OK");
    let merged: i64 = q(&mut c, &["PFCOUNT", "dst"]).await.unwrap();
    let bound = hll_bound(7500.0);
    assert!(
        (merged as f64 - 7500.0).abs() <= bound,
        "merged estimate {merged} outside ±{bound:.1} of 7500"
    );

    // The merged sketch behaves like one built directly from the union.
    pfadd_range(&mut c, "ref", 0, 7500).await;
    let reference: i64 = q(&mut c, &["PFCOUNT", "ref"]).await.unwrap();
    assert!(
        (reference as f64 - 7500.0).abs() <= bound,
        "reference estimate {reference} outside bound"
    );

    let ty: String = q(&mut c, &["TYPE", "dst"]).await.unwrap();
    assert_eq!(ty, "string");

    // Empty source list clears the destination.
    let ok: String = q(&mut c, &["PFMERGE", "dst"]).await.unwrap();
    assert_eq!(ok, "OK");
    assert_eq!(q::<i64>(&mut c, &["PFCOUNT", "dst"]).await.unwrap(), 0);

    // Multi-key PFCOUNT unions the inputs.
    let multi: i64 = q(&mut c, &["PFCOUNT", "s1", "s2"]).await.unwrap();
    assert!(
        (multi as f64 - 7500.0).abs() <= bound,
        "multi-key count {multi} outside bound"
    );
    lab.stop();
}

#[tokio::test]
async fn hll_wrongtype_on_plain_string() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;
    let _: String = q(&mut c, &["SET", "s", "v"]).await.unwrap();
    let e = qerr(&mut c, &["PFADD", "s", "x"]).await;
    assert!(e.contains("WRONGTYPE"), "{e}");
    let e = qerr(&mut c, &["PFCOUNT", "s"]).await;
    assert!(e.contains("WRONGTYPE"), "{e}");
    lab.stop();
}

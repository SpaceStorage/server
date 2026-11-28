//! Redis `bitmap` family (`017` US5 T061). Bitmaps ride the L0 `bitmap`
//! primitive as an `EntryPayload::Bitmap` inside the namespace key-space
//! container; every access goes through `session.load_entry`/`store_entry`
//! (never `session.catalog`). `TYPE` reports `string` via `EntryPayload::visible`.

use bytes::Bytes;

use crate::lower::{BitmapValue, EntryPayload, RedisEntry};
use crate::render;
use super::{RedisReply, SessionState};

fn fresh_bitmap_entry(key: &str) -> RedisEntry {
    RedisEntry::new(
        Bytes::from(key.as_bytes().to_vec()),
        EntryPayload::Bitmap(BitmapValue(Vec::new())),
    )
}

/// Load one key as bitmap bytes. A plain string payload is a valid bitmap
/// (Redis strings are bitmaps); any other kind is `WRONGTYPE`.
fn load_bitmap(
    session: &SessionState,
    id: spacestorage_types::ContainerId,
    key: &str,
) -> Result<Option<(RedisEntry, Vec<u8>)>, RedisReply> {
    match session.load_entry(id, key)? {
        None => Ok(None),
        Some(mut e) => match std::mem::replace(&mut e.payload, EntryPayload::Bytes(Vec::new())) {
            EntryPayload::Bitmap(b) => Ok(Some((e, b.0))),
            EntryPayload::Bytes(b) => Ok(Some((e, b))),
            other => {
                e.payload = other;
                Err(render::wrongtype())
            }
        },
    }
}

pub fn dispatch(session: &mut SessionState, upper: &str, args: &[&str]) -> Option<RedisReply> {
    match upper {
        "SETBIT" => Some(cmd_setbit(session, args)),
        "GETBIT" => Some(cmd_getbit(session, args)),
        "BITCOUNT" => Some(cmd_bitcount(session, args)),
        "BITPOS" => Some(cmd_bitpos(session, args)),
        "BITOP" => Some(cmd_bitop(session, args)),
        "BITFIELD" => Some(cmd_bitfield(session, args, false)),
        "BITFIELD_RO" => Some(cmd_bitfield(session, args, true)),
        _ => None,
    }
}

// ---- bit primitives (MSB-first, Redis bit numbering) -----------------------

fn get_bit(bytes: &[u8], pos: usize) -> u64 {
    let byte = pos / 8;
    if byte >= bytes.len() {
        return 0;
    }
    u64::from(bytes[byte] & (0x80u8 >> (pos % 8)) != 0)
}

fn set_bit(bytes: &mut [u8], pos: usize, value: bool) {
    let byte = pos / 8;
    let mask = 0x80u8 >> (pos % 8);
    if value {
        bytes[byte] |= mask;
    } else {
        bytes[byte] &= !mask;
    }
}

fn parse_offset(s: &str) -> Option<u64> {
    // Redis caps bit offsets at `2^32 - 1` (a 4 GiB string); an offset at or
    // above `2^32` is rejected as out of range before any size-limit check.
    let v = s.parse::<u64>().ok()?;
    (v < (1u64 << 32)).then_some(v)
}

fn offset_error() -> RedisReply {
    render::error("ERR bit offset is not an integer or out of range")
}

// ---- SETBIT / GETBIT -------------------------------------------------------

fn cmd_setbit(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 3 {
        return render::arity("setbit");
    }
    let (key, Some(offset)) = (args[0], parse_offset(args[1])) else {
        return offset_error();
    };
    let bit = match args[2] {
        "0" => false,
        "1" => true,
        _ => return render::error("ERR bit is not an integer or out of range"),
    };
    let need = offset / 8 + 1;
    let limits = session.limit_services.limits();
    if let Err(e) = limits.check_key(key.len() as u64) {
        return render::limit(&e.to_string());
    }
    // Reject an over-limit resulting length before mutating (015 limit).
    if let Err(e) = limits.check_value(need) {
        return render::limit(&e.to_string());
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (mut entry, mut bytes) = match load_bitmap(session, id, key) {
        Ok(Some(v)) => v,
        Ok(None) => (fresh_bitmap_entry(key), Vec::new()),
        Err(e) => return e,
    };
    let need = need as usize;
    if bytes.len() < need {
        bytes.resize(need, 0);
    }
    let old = get_bit(&bytes, offset as usize);
    set_bit(&mut bytes, offset as usize, bit);
    entry.replace(EntryPayload::Bitmap(BitmapValue(bytes)));
    match session.store_entry(id, &entry) {
        Ok(()) => RedisReply::Integer(old as i64),
        Err(e) => e,
    }
}

fn cmd_getbit(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 2 {
        return render::arity("getbit");
    }
    let Some(offset) = parse_offset(args[1]) else {
        return offset_error();
    };
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    match load_bitmap(session, id, args[0]) {
        Ok(Some((_, bytes))) => RedisReply::Integer(get_bit(&bytes, offset as usize) as i64),
        Ok(None) => RedisReply::Integer(0),
        Err(e) => e,
    }
}

// ---- BITCOUNT --------------------------------------------------------------

enum Unit {
    Byte,
    Bit,
}

fn parse_unit(arg: &str) -> Option<Unit> {
    match arg.to_ascii_uppercase().as_str() {
        "BYTE" => Some(Unit::Byte),
        "BIT" => Some(Unit::Bit),
        _ => None,
    }
}

/// Normalise an inclusive `[start, end]` range over `total` units.
fn norm_range(start: i64, end: i64, total: i64) -> Option<(usize, usize)> {
    if total <= 0 {
        return None;
    }
    let mut s = if start < 0 { start + total } else { start };
    let mut e = if end < 0 { end + total } else { end };
    if s < 0 {
        s = 0;
    }
    if e >= total {
        e = total - 1;
    }
    if s > e || s >= total {
        return None;
    }
    Some((s as usize, e as usize))
}

fn cmd_bitcount(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if !matches!(args.len(), 1 | 3 | 4) {
        return render::arity("bitcount");
    }
    let unit = if args.len() == 4 {
        match parse_unit(args[3]) {
            Some(u) => u,
            None => return render::error("ERR syntax error"),
        }
    } else {
        Unit::Byte
    };
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let bytes = match load_bitmap(session, id, args[0]) {
        Ok(Some((_, b))) => b,
        Ok(None) => return RedisReply::Integer(0),
        Err(e) => return e,
    };
    if args.len() == 1 {
        let n: u64 = bytes.iter().map(|b| u64::from(b.count_ones())).sum();
        return RedisReply::Integer(n as i64);
    }
    let (Some(start), Some(end)) = (args[1].parse::<i64>().ok(), args[2].parse::<i64>().ok()) else {
        return render::error("ERR value is not an integer or out of range");
    };
    let n = match unit {
        Unit::Byte => match norm_range(start, end, bytes.len() as i64) {
            Some((s, e)) => bytes[s..=e].iter().map(|b| u64::from(b.count_ones())).sum(),
            None => 0,
        },
        Unit::Bit => {
            let total_bits = (bytes.len() * 8) as i64;
            match norm_range(start, end, total_bits) {
                Some((lo, hi)) => (lo..=hi).map(|p| get_bit(&bytes, p)).sum(),
                None => 0,
            }
        }
    };
    RedisReply::Integer(n as i64)
}

// ---- BITPOS ----------------------------------------------------------------

fn cmd_bitpos(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if !(2..=5).contains(&args.len()) {
        return render::arity("bitpos");
    }
    let target = match args[1] {
        "0" => 0u64,
        "1" => 1u64,
        _ => return render::error("ERR The bit argument must be 1 or 0."),
    };
    let unit = if args.len() == 5 {
        match parse_unit(args[4]) {
            Some(u) => u,
            None => return render::error("ERR syntax error"),
        }
    } else {
        Unit::Byte
    };
    let has_start = args.len() >= 3;
    let has_end = args.len() >= 4;
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let bytes = match load_bitmap(session, id, args[0]) {
        Ok(Some((_, b))) => b,
        Ok(None) => Vec::new(),
        Err(e) => return e,
    };
    let total_bits = (bytes.len() * 8) as i64;
    let (unit_total, bits_per_unit) = match unit {
        Unit::Byte => (bytes.len() as i64, 8i64),
        Unit::Bit => (total_bits, 1i64),
    };
    let start_arg = if has_start {
        match args[2].parse::<i64>().ok() {
            Some(v) => v,
            None => return render::error("ERR value is not an integer or out of range"),
        }
    } else {
        0
    };
    let end_arg = if has_end {
        match args[3].parse::<i64>().ok() {
            Some(v) => v,
            None => return render::error("ERR value is not an integer or out of range"),
        }
    } else {
        unit_total - 1
    };
    let range = norm_range(start_arg, end_arg, unit_total);
    let result = match range {
        Some((s, e)) => {
            let lo = s as i64 * bits_per_unit;
            let hi = e as i64 * bits_per_unit + (bits_per_unit - 1);
            let mut found = None;
            for p in lo..=hi {
                if get_bit(&bytes, p as usize) == target {
                    found = Some(p);
                    break;
                }
            }
            match found {
                Some(p) => Some(p),
                // Redis: a clear-bit scan with no explicit end reports the total
                // bit length; otherwise a miss is -1.
                None if target == 0 && !has_end => Some(total_bits),
                None => None,
            }
        }
        None => {
            if target == 0 && !has_end {
                Some(total_bits)
            } else {
                None
            }
        }
    };
    RedisReply::Integer(result.unwrap_or(-1))
}

// ---- BITOP -----------------------------------------------------------------

fn cmd_bitop(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() < 3 {
        return render::arity("bitop");
    }
    let op = args[0].to_ascii_uppercase();
    let dest = args[1];
    let sources = &args[2..];
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let mut loaded = Vec::with_capacity(sources.len());
    for k in sources {
        match load_bitmap(session, id, k) {
            Ok(Some((_, b))) => loaded.push(b),
            Ok(None) => loaded.push(Vec::new()),
            Err(e) => return e,
        }
    }
    let result: Vec<u8> = match op.as_str() {
        "NOT" => {
            if sources.len() != 1 {
                return render::error("ERR BITOP NOT must be called with a single source key.");
            }
            loaded[0].iter().map(|b| !b).collect()
        }
        "AND" | "OR" | "XOR" => {
            let maxlen = loaded.iter().map(Vec::len).max().unwrap_or(0);
            let mut out = Vec::with_capacity(maxlen);
            for i in 0..maxlen {
                let mut acc: u8 = if op == "AND" { 0xFF } else { 0x00 };
                for s in &loaded {
                    let b = s.get(i).copied().unwrap_or(0);
                    match op.as_str() {
                        "AND" => acc &= b,
                        "OR" => acc |= b,
                        _ => acc ^= b,
                    }
                }
                out.push(acc);
            }
            out
        }
        _ => return render::error("ERR syntax error"),
    };
    let limits = session.limit_services.limits();
    if let Err(e) = limits.check_value(result.len() as u64) {
        return render::limit(&e.to_string());
    }
    if result.is_empty() {
        // All sources empty/missing: Redis deletes the destination and returns 0.
        if let Err(e) = session.delete_entry(id, dest) {
            return e;
        }
        return RedisReply::Integer(0);
    }
    let mut entry = fresh_bitmap_entry(dest);
    entry.replace(EntryPayload::Bitmap(BitmapValue(result.clone())));
    match session.store_entry(id, &entry) {
        Ok(()) => RedisReply::Integer(result.len() as i64),
        Err(e) => e,
    }
}

// ---- BITFIELD / BITFIELD_RO ------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Overflow {
    Wrap,
    Sat,
    Fail,
}

fn invalid_type() -> RedisReply {
    render::error(
        "ERR Invalid bitfield type. Use something like i16 u8. Note that u64 is not supported but i64 is.",
    )
}

/// Parse `iN`/`uN` (signed 1..=64, unsigned 1..=63).
fn parse_bf_type(s: &str) -> Result<(bool, u32), RedisReply> {
    let bytes = s.as_bytes();
    if bytes.len() < 2 {
        return Err(invalid_type());
    }
    let signed = match bytes[0] {
        b'i' => true,
        b'u' => false,
        _ => return Err(invalid_type()),
    };
    let width: u32 = s[1..].parse().map_err(|_| invalid_type())?;
    let ok = if signed {
        (1..=64).contains(&width)
    } else {
        (1..=63).contains(&width)
    };
    if !ok {
        return Err(invalid_type());
    }
    Ok((signed, width))
}

/// Resolve a `GET`/`SET`/`INCRBY` offset to an absolute (non-negative) bit
/// offset. `#N` counts fields; a negative value addresses from the end.
fn resolve_offset(spec: &str, width: u32, byte_len: usize) -> Option<i128> {
    let total_bits = byte_len as i128 * 8;
    let off = if let Some(rest) = spec.strip_prefix('#') {
        let n: i64 = rest.parse().ok()?;
        let units = total_bits / width as i128;
        if n < 0 {
            (units + i128::from(n)) * width as i128
        } else {
            i128::from(n) * width as i128
        }
    } else {
        let n: i64 = spec.parse().ok()?;
        if n < 0 {
            total_bits + i128::from(n)
        } else {
            i128::from(n)
        }
    };
    // Redis caps bit offsets at `2^32 - 1` (a 4 GiB string); an offset at or
    // above `2^32` is out of range. This also keeps every later `usize`/length
    // computation well-defined.
    if off < 0 || off >= (1i128 << 32) {
        return None;
    }
    Some(off)
}

fn field_bounds(signed: bool, width: u32) -> (i128, i128) {
    if signed {
        let max = (1i128 << (width - 1)) - 1;
        (-(1i128 << (width - 1)), max)
    } else {
        (0, (1i128 << width) - 1)
    }
}

fn apply_overflow(value: i128, min: i128, max: i128, mode: Overflow) -> Option<i128> {
    if value >= min && value <= max {
        return Some(value);
    }
    match mode {
        Overflow::Wrap => {
            let range = max - min + 1;
            let mut x = (value - min) % range;
            if x < 0 {
                x += range;
            }
            Some(x + min)
        }
        Overflow::Sat => Some(value.clamp(min, max)),
        Overflow::Fail => None,
    }
}

fn read_unsigned(bytes: &[u8], bit_off: usize, width: u32) -> u64 {
    let mut v = 0u64;
    for i in 0..width {
        v = (v << 1) | get_bit(bytes, bit_off + i as usize);
    }
    v
}

fn write_unsigned(bytes: &mut [u8], bit_off: usize, width: u32, value: u64) {
    for i in 0..width {
        let bit = (value >> (width - 1 - i)) & 1 != 0;
        set_bit(bytes, bit_off + i as usize, bit);
    }
}

fn sign_extend(raw: u64, width: u32) -> i64 {
    if width >= 64 {
        return raw as i64;
    }
    let sign_bit = 1u64 << (width - 1);
    if raw & sign_bit != 0 {
        (i128::from(raw) - (1i128 << width)) as i64
    } else {
        raw as i64
    }
}

fn encode_low_bits(value: i128, width: u32) -> u64 {
    let mask: u128 = if width >= 128 {
        u128::MAX
    } else {
        (1u128 << width) - 1
    };
    (value as u128 & mask) as u64
}

fn cmd_bitfield(session: &mut SessionState, args: &[&str], read_only: bool) -> RedisReply {
    let name = if read_only { "bitfield_ro" } else { "bitfield" };
    if args.is_empty() {
        return render::arity(name);
    }
    let key = args[0];
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (mut entry, mut bytes) = match load_bitmap(session, id, key) {
        Ok(Some(v)) => v,
        Ok(None) => (fresh_bitmap_entry(key), Vec::new()),
        Err(e) => return e,
    };
    let mut overflow = Overflow::Wrap;
    let mut dirty = false;
    let mut out: Vec<RedisReply> = Vec::new();
    let mut i = 1;
    while i < args.len() {
        let sub = args[i].to_ascii_uppercase();
        match sub.as_str() {
            "GET" => {
                if i + 2 >= args.len() {
                    return render::error("ERR syntax error");
                }
                let (signed, width) = match parse_bf_type(args[i + 1]) {
                    Ok(v) => v,
                    Err(e) => return e,
                };
                let Some(bit_off) = resolve_offset(args[i + 2], width, bytes.len()) else {
                    return offset_error();
                };
                let raw = read_unsigned(&bytes, bit_off as usize, width);
                let val = if signed {
                    sign_extend(raw, width)
                } else {
                    raw as i64
                };
                out.push(RedisReply::Integer(val));
                i += 3;
            }
            "SET" | "INCRBY" => {
                if read_only {
                    return render::error("ERR BITFIELD_RO only supports the GET subcommand");
                }
                if i + 3 >= args.len() {
                    return render::error("ERR syntax error");
                }
                let (signed, width) = match parse_bf_type(args[i + 1]) {
                    Ok(v) => v,
                    Err(e) => return e,
                };
                let Some(bit_off) = resolve_offset(args[i + 2], width, bytes.len()) else {
                    return offset_error();
                };
                let arg: i64 = match args[i + 3].parse() {
                    Ok(v) => v,
                    Err(_) => return render::error("ERR value is not an integer or out of range"),
                };
                let (min, max) = field_bounds(signed, width);
                let candidate = if sub == "SET" {
                    i128::from(arg)
                } else {
                    i128::from(read_unsigned(&bytes, bit_off as usize, width) as i64) + i128::from(arg)
                };
                match apply_overflow(candidate, min, max, overflow) {
                    None => out.push(RedisReply::Bulk(None)),
                    Some(value) => {
                        let need_bytes = ((bit_off as usize) + width as usize + 7) / 8;
                        if bytes.len() < need_bytes {
                            let limits = session.limit_services.limits();
                            if let Err(e) = limits.check_value(need_bytes as u64) {
                                return render::limit(&e.to_string());
                            }
                            bytes.resize(need_bytes, 0);
                        }
                        write_unsigned(
                            &mut bytes,
                            bit_off as usize,
                            width,
                            encode_low_bits(value, width),
                        );
                        out.push(RedisReply::Integer(value as i64));
                        dirty = true;
                    }
                }
                i += 4;
            }
            "OVERFLOW" => {
                if read_only {
                    return render::error("ERR BITFIELD_RO only supports the GET subcommand");
                }
                if i + 1 >= args.len() {
                    return render::error("ERR syntax error");
                }
                overflow = match args[i + 1].to_ascii_uppercase().as_str() {
                    "WRAP" => Overflow::Wrap,
                    "SAT" => Overflow::Sat,
                    "FAIL" => Overflow::Fail,
                    _ => {
                        return render::error(
                            "ERR Invalid OVERFLOW type specified. Use WRAP, SAT or FAIL.",
                        )
                    }
                };
                i += 2;
            }
            _ => return render::error("ERR syntax error"),
        }
    }
    if dirty {
        entry.replace(EntryPayload::Bitmap(BitmapValue(bytes)));
        if let Err(e) = session.store_entry(id, &entry) {
            return e;
        }
    }
    RedisReply::Array(out)
}

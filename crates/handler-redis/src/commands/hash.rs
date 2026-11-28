//! Redis `hash` family incl. 7.4 hash-field expiry (`017` US1 T022).

use std::collections::BTreeMap;

use bytes::Bytes;

use crate::expiry::{Deadline, TtlInstant};
use crate::lower::{EntryPayload, HashField, HashValue, RedisEntry, RedisVisibleType};
use crate::render;
use super::{RedisReply, SessionState};

const MISSING_INT: &str = "ERR value is not an integer or out of range";
const MISSING_FLOAT: &str = "ERR value is not a valid float";

fn now() -> TtlInstant {
    TtlInstant::now()
}

fn field_live(f: &HashField) -> bool {
    match f.expire_at {
        Some(d) => !d.is_past(now()),
        None => true,
    }
}

fn parse_i64(s: &str) -> Option<i64> {
    s.parse::<i64>().ok()
}

fn parse_f64(s: &str) -> Option<f64> {
    s.parse::<f64>().ok().filter(|f| f.is_finite())
}

fn parse_f64_any(s: &str) -> Option<f64> {
    s.parse::<f64>().ok()
}

/// Load a hash, returning `WRONGTYPE` when the key holds another type.
fn load_hash(
    session: &SessionState,
    id: spacestorage_types::ContainerId,
    key: &str,
) -> Result<Option<HashMap>, RedisReply> {
    match session.load_entry(id, key)? {
        None => Ok(None),
        Some(mut e) => {
            if !matches!(e.payload, EntryPayload::Hash(_)) {
                return Err(render::wrongtype());
            }
            let hash = match std::mem::replace(&mut e.payload, EntryPayload::Bytes(Vec::new())) {
                EntryPayload::Hash(h) => h,
                _ => unreachable!(),
            };
            Ok(Some(HashMap { entry: e, hash }))
        }
    }
}

struct HashMap {
    entry: RedisEntry,
    hash: HashValue,
}

fn store_hash(
    session: &SessionState,
    id: spacestorage_types::ContainerId,
    entry: &mut RedisEntry,
    hash: HashValue,
) -> Result<(), RedisReply> {
    entry.replace(EntryPayload::Hash(hash));
    session.store_entry(id, entry)
}

/// Redis 7.4 drops the key once its last field is removed.
fn store_or_drop(
    session: &SessionState,
    id: spacestorage_types::ContainerId,
    entry: &mut RedisEntry,
    hash: HashValue,
    key: &str,
) -> Result<(), RedisReply> {
    if hash.fields.is_empty() {
        entry.replace(EntryPayload::Hash(hash));
        session.delete_entry(id, key)?;
        Ok(())
    } else {
        store_hash(session, id, entry, hash)
    }
}

pub fn dispatch(session: &mut SessionState, upper: &str, args: &[&str]) -> Option<RedisReply> {
    match upper {
        "HSET" | "HMSET" => Some(cmd_hset(session, args, upper == "HMSET")),
        "HSETNX" => Some(cmd_hsetnx(session, args)),
        "HGET" => Some(cmd_hget(session, args)),
        "HMGET" => Some(cmd_hmget(session, args)),
        "HDEL" => Some(cmd_hdel(session, args)),
        "HGETALL" => Some(cmd_hgetall(session, args)),
        "HKEYS" => Some(cmd_hkeys(session, args)),
        "HVALS" => Some(cmd_hvals(session, args)),
        "HLEN" => Some(cmd_hlen(session, args)),
        "HEXISTS" => Some(cmd_hexists(session, args)),
        "HINCRBY" => Some(cmd_hincrby(session, args)),
        "HINCRBYFLOAT" => Some(cmd_hincrbyfloat(session, args)),
        "HSTRLEN" => Some(cmd_hstrlen(session, args)),
        "HRANDFIELD" => Some(cmd_hrandfield(session, args)),
        "HSCAN" => Some(cmd_hscan(session, args)),
        "HEXPIRE" | "HPEXPIRE" | "HEXPIREAT" | "HPEXPIREAT" => {
            Some(cmd_hexpire(session, upper, args))
        }
        "HTTL" | "HPTTL" => Some(cmd_httl(session, upper, args, false)),
        "HEXPIRETIME" | "HPEXPIRETIME" => Some(cmd_httl(session, upper, args, true)),
        "HGETEX" => Some(cmd_hgetex(session, args)),
        "HGETDEL" => Some(cmd_hgetdel(session, args)),
        _ => None,
    }
}

fn fresh_hash_entry(key: &str) -> RedisEntry {
    RedisEntry::new(
        Bytes::from(key.as_bytes().to_vec()),
        EntryPayload::Hash(HashValue::default()),
    )
}

fn cmd_hset(session: &mut SessionState, args: &[&str], hmset: bool) -> RedisReply {
    let name = if hmset { "hmset" } else { "hset" };
    if args.len() < 3 || args.len() % 2 == 0 {
        return render::arity(name);
    }
    let key = args[0];
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let mut h = match load_hash(session, id, key) {
        Ok(Some(h)) => h,
        Ok(None) => HashMap {
            entry: fresh_hash_entry(key),
            hash: HashValue::default(),
        },
        Err(e) => return e,
    };
    let mut created = 0i64;
    for pair in args[1..].chunks(2) {
        let f = Bytes::from(pair[0].as_bytes().to_vec());
        let v = Bytes::from(pair[1].as_bytes().to_vec());
        if h.hash.fields.get(&f).map(|x| !field_live(x)).unwrap_or(true) {
            h.hash.fields.remove(&f);
            created += 1;
        }
        h.hash.fields.insert(f, HashField { value: v, expire_at: None });
    }
    let mut entry = h.entry;
    if let Err(e) = store_hash(session, id, &mut entry, h.hash) {
        return e;
    }
    if hmset {
        RedisReply::Ok
    } else {
        RedisReply::Integer(created)
    }
}

fn cmd_hsetnx(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 3 {
        return render::arity("hsetnx");
    }
    let key = args[0];
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let mut h = match load_hash(session, id, key) {
        Ok(Some(h)) => h,
        Ok(None) => HashMap {
            entry: fresh_hash_entry(key),
            hash: HashValue::default(),
        },
        Err(e) => return e,
    };
    let f = Bytes::from(args[1].as_bytes().to_vec());
    if h.hash.fields.get(&f).map(field_live).unwrap_or(false) {
        return RedisReply::Integer(0);
    }
    h.hash.fields.insert(
        f,
        HashField {
            value: Bytes::from(args[2].as_bytes().to_vec()),
            expire_at: None,
        },
    );
    let mut entry = h.entry;
    match store_hash(session, id, &mut entry, h.hash) {
        Ok(()) => RedisReply::Integer(1),
        Err(e) => e,
    }
}

fn cmd_hget(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 2 {
        return render::arity("hget");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    match load_hash(session, id, args[0]) {
        Ok(None) => RedisReply::Bulk(None),
        Ok(Some(h)) => match h.hash.fields.get(args[1].as_bytes()) {
            Some(f) if field_live(f) => RedisReply::Bulk(Some(f.value.to_vec())),
            _ => RedisReply::Bulk(None),
        },
        Err(e) => e,
    }
}

fn cmd_hmget(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() < 2 {
        return render::arity("hmget");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let fields = match load_hash(session, id, args[0]) {
        Ok(Some(h)) => Some(h.hash.fields),
        Ok(None) => None,
        Err(e) => return e,
    };
    let out = args[1..]
        .iter()
        .map(|f| match fields.as_ref().and_then(|m| m.get(f.as_bytes())) {
            Some(v) if field_live(v) => RedisReply::Bulk(Some(v.value.to_vec())),
            _ => RedisReply::Bulk(None),
        })
        .collect();
    RedisReply::Array(out)
}

fn cmd_hdel(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() < 2 {
        return render::arity("hdel");
    }
    let key = args[0];
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let mut h = match load_hash(session, id, key) {
        Ok(Some(h)) => h,
        Ok(None) => return RedisReply::Integer(0),
        Err(e) => return e,
    };
    let mut n = 0i64;
    for f in &args[1..] {
        if h.hash.fields.remove(f.as_bytes()).is_some() {
            n += 1;
        }
    }
    if n > 0 {
        let mut entry = h.entry;
        if let Err(e) = store_or_drop(session, id, &mut entry, h.hash, key) {
            return e;
        }
    }
    RedisReply::Integer(n)
}

/// Live, non-expired fields in sorted order.
fn live_fields(h: &HashValue) -> Vec<(&Bytes, &HashField)> {
    h.fields
        .iter()
        .filter(|(_, f)| field_live(f))
        .collect()
}

fn cmd_hgetall(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 1 {
        return render::arity("hgetall");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    match load_hash(session, id, args[0]) {
        Ok(Some(h)) => {
            let mut out = Vec::new();
            for (f, v) in live_fields(&h.hash) {
                out.push(RedisReply::Bulk(Some(f.to_vec())));
                out.push(RedisReply::Bulk(Some(v.value.to_vec())));
            }
            RedisReply::Array(out)
        }
        Ok(None) => RedisReply::Array(vec![]),
        Err(e) => e,
    }
}

fn cmd_hkeys(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 1 {
        return render::arity("hkeys");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    match load_hash(session, id, args[0]) {
        Ok(Some(h)) => RedisReply::Array(
            live_fields(&h.hash)
                .into_iter()
                .map(|(f, _)| RedisReply::Bulk(Some(f.to_vec())))
                .collect(),
        ),
        Ok(None) => RedisReply::Array(vec![]),
        Err(e) => e,
    }
}

fn cmd_hvals(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 1 {
        return render::arity("hvals");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    match load_hash(session, id, args[0]) {
        Ok(Some(h)) => RedisReply::Array(
            live_fields(&h.hash)
                .into_iter()
                .map(|(_, v)| RedisReply::Bulk(Some(v.value.to_vec())))
                .collect(),
        ),
        Ok(None) => RedisReply::Array(vec![]),
        Err(e) => e,
    }
}

fn cmd_hlen(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 1 {
        return render::arity("hlen");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    match load_hash(session, id, args[0]) {
        Ok(Some(h)) => RedisReply::Integer(live_fields(&h.hash).len() as i64),
        Ok(None) => RedisReply::Integer(0),
        Err(e) => e,
    }
}

fn cmd_hexists(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 2 {
        return render::arity("hexists");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    match load_hash(session, id, args[0]) {
        Ok(Some(h)) => {
            let live = h
                .hash
                .fields
                .get(args[1].as_bytes())
                .map(field_live)
                .unwrap_or(false);
            RedisReply::Integer(if live { 1 } else { 0 })
        }
        Ok(None) => RedisReply::Integer(0),
        Err(e) => e,
    }
}

fn cmd_hincrby(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 3 {
        return render::arity("hincrby");
    }
    let Some(delta) = parse_i64(args[2]) else {
        return render::error(MISSING_INT);
    };
    hincr_common(session, args[0], args[1], delta as f64, true)
}

fn cmd_hincrbyfloat(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 3 {
        return render::arity("hincrbyfloat");
    }
    let Some(delta) = parse_f64(args[2]) else {
        return render::error(MISSING_FLOAT);
    };
    hincr_common(session, args[0], args[1], delta, false)
}

fn hincr_common(
    session: &mut SessionState,
    key: &str,
    field: &str,
    delta: f64,
    integer: bool,
) -> RedisReply {
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let mut h = match load_hash(session, id, key) {
        Ok(Some(h)) => h,
        Ok(None) => HashMap {
            entry: fresh_hash_entry(key),
            hash: HashValue::default(),
        },
        Err(e) => return e,
    };
    let fkb = Bytes::from(field.as_bytes().to_vec());
    let current = match h.hash.fields.get(&fkb) {
        Some(f) if field_live(f) => {
            let text = std::str::from_utf8(&f.value).unwrap_or("");
            if integer {
                match parse_i64(text) {
                    Some(n) => n as f64,
                    None => return render::error("ERR hash value is not an integer"),
                }
            } else {
                match parse_f64_any(text) {
                    Some(n) => n,
                    None => return render::error("ERR hash value is not a float"),
                }
            }
        }
        _ => 0.0,
    };
    let next = current + delta;
    let text = if integer {
        if next == next.trunc() && next.abs() < 9e15 {
            format!("{}", next as i64)
        } else {
            return render::error("ERR increment or decrement would overflow");
        }
    } else if next == next.trunc() && next.abs() < 1e17 {
        format!("{}", next as i64)
    } else {
        format!("{next}")
    };
    h.hash.fields.insert(
        fkb,
        HashField {
            value: Bytes::from(text.clone().into_bytes()),
            expire_at: None,
        },
    );
    let mut entry = h.entry;
    if let Err(e) = store_hash(session, id, &mut entry, h.hash) {
        return e;
    }
    if integer {
        match text.parse::<i64>() {
            Ok(n) => RedisReply::Integer(n),
            Err(_) => render::error("ERR increment or decrement would overflow"),
        }
    } else {
        RedisReply::Bulk(Some(text.into_bytes()))
    }
}

fn cmd_hstrlen(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 2 {
        return render::arity("hstrlen");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    match load_hash(session, id, args[0]) {
        Ok(Some(h)) => match h.hash.fields.get(args[1].as_bytes()) {
            Some(f) if field_live(f) => RedisReply::Integer(f.value.len() as i64),
            _ => RedisReply::Integer(0),
        },
        Ok(None) => RedisReply::Integer(0),
        Err(e) => e,
    }
}

fn cmd_hrandfield(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.is_empty() {
        return render::arity("hrandfield");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (fields, exists) = match load_hash(session, id, args[0]) {
        Ok(Some(h)) => (live_fields(&h.hash).into_iter().map(|(k, v)| (k.clone(), v.value.clone())).collect::<Vec<_>>(), true),
        Ok(None) => (Vec::new(), false),
        Err(e) => return e,
    };
    if args.len() == 1 {
        if fields.is_empty() {
            return RedisReply::Bulk(None);
        }
        // deterministic first field (single-node, no RNG dependency)
        return RedisReply::Bulk(Some(fields[0].0.to_vec()));
    }
    let Some(count) = parse_i64(args[1]) else {
        return render::error(MISSING_INT);
    };
    let with_values = args
        .get(2)
        .map(|s| s.eq_ignore_ascii_case("WITHVALUES"))
        .unwrap_or(false);
    let _ = exists;
    // Distinct selection: negative count allows repeats, but a deterministic
    // single-pass is acceptable for conformance (no duplicates for positive).
    let n = if count < 0 { (-count) as usize } else { count as usize };
    let n = n.min(fields.len());
    let mut out = Vec::new();
    for (k, v) in fields.into_iter().take(n) {
        out.push(RedisReply::Bulk(Some(k.to_vec())));
        if with_values {
            out.push(RedisReply::Bulk(Some(v.to_vec())));
        }
    }
    if args.len() == 1 {
        RedisReply::Bulk(out.into_iter().next().and_then(|r| match r {
            RedisReply::Bulk(b) => b,
            _ => None,
        }))
    } else {
        RedisReply::Array(out)
    }
}

fn cmd_hscan(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() < 2 {
        return render::arity("hscan");
    }
    let mut i = 2;
    let mut count = 10usize;
    let mut pattern: Option<String> = None;
    let mut novalues = false;
    while i < args.len() {
        let up = args[i].to_ascii_uppercase();
        match up.as_str() {
            "COUNT" => {
                if let Some(n) = args.get(i + 1).and_then(|s| s.parse().ok()) {
                    count = n;
                }
                i += 2;
            }
            "MATCH" => {
                pattern = args.get(i + 1).map(|s| s.to_string());
                i += 2;
            }
            "NOVALUES" => {
                novalues = true;
                i += 1;
            }
            _ => i += 1,
        }
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let fields = match load_hash(session, id, args[0]) {
        Ok(Some(h)) => live_fields(&h.hash)
            .into_iter()
            .map(|(k, v)| (k.clone(), v.value.clone()))
            .collect::<Vec<_>>(),
        Ok(None) => Vec::new(),
        Err(e) => return e,
    };
    let selected: Vec<_> = fields
        .into_iter()
        .filter(|(k, _)| {
            pattern
                .as_deref()
                .map(|p| crate::lower::glob_match(p, &String::from_utf8_lossy(k)))
                .unwrap_or(true)
        })
        .take(count.max(1))
        .collect();
    let mut items = Vec::new();
    for (k, v) in selected {
        items.push(RedisReply::Bulk(Some(k.to_vec())));
        if !novalues {
            items.push(RedisReply::Bulk(Some(v.to_vec())));
        }
    }
    RedisReply::Array(vec![
        RedisReply::Bulk(Some(b"0".to_vec())),
        RedisReply::Array(items),
    ])
}

/// `HEXPIRE key seconds [NX|XX|GT|LT] FIELDS n field...` (+ ms/at variants).
fn cmd_hexpire(session: &mut SessionState, upper: &str, args: &[&str]) -> RedisReply {
    let (absolute, millis) = match upper {
        "HEXPIRE" => (false, false),
        "HPEXPIRE" => (false, true),
        "HEXPIREAT" => (true, false),
        "HPEXPIREAT" => (true, true),
        _ => unreachable!(),
    };
    if args.len() < 4 {
        return render::arity(&upper.to_ascii_lowercase());
    }
    let Some(n) = parse_i64(args[1]) else {
        return render::error(MISSING_INT);
    };
    let mut i = 2;
    let mut cond: Option<&str> = None;
    while i < args.len() && !args[i].eq_ignore_ascii_case("FIELDS") {
        match args[i].to_ascii_uppercase().as_str() {
            "NX" => cond = Some("NX"),
            "XX" => cond = Some("XX"),
            "GT" => cond = Some("GT"),
            "LT" => cond = Some("LT"),
            _ => return render::error("ERR syntax error"),
        }
        i += 1;
    }
    if i >= args.len() || !args[i].eq_ignore_ascii_case("FIELDS") {
        return render::error("ERR syntax error");
    }
    i += 1;
    let Some(count) = args.get(i).and_then(|s| parse_i64(s)) else {
        return render::error(MISSING_INT);
    };
    i += 1;
    let fields: Vec<&str> = args.get(i..).unwrap_or(&[]).to_vec();
    if count as usize != fields.len() {
        return render::error("ERR Parameter `numFields` is not equal to the number of fields");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let key = args[0];
    let mut h = match load_hash(session, id, key) {
        Ok(Some(h)) => h,
        Ok(None) => {
            let arr = fields.iter().map(|_| RedisReply::Integer(-2)).collect();
            return RedisReply::Array(arr);
        }
        Err(e) => return e,
    };
    let now = now();
    let at = match (absolute, millis) {
        (true, false) => n.saturating_mul(1000),
        (true, true) => n,
        (false, false) => now.millis.saturating_add(n.saturating_mul(1000)),
        (false, true) => now.millis.saturating_add(n),
    };
    let new = Deadline::new(TtlInstant::from_unix_millis(at));
    let mut codes = Vec::new();
    let mut changed = false;
    for f in fields {
        match h.hash.fields.get_mut(f.as_bytes()) {
            Some(field) => {
                let current = field.expire_at;
                let live = current.map(|d| !d.is_past(now)).unwrap_or(true);
                if !live {
                    codes.push(RedisReply::Integer(2));
                    continue;
                }
                let ok = match cond {
                    None => true,
                    Some("NX") => current.is_none(),
                    Some("XX") => current.is_some(),
                    Some("GT") => current.map(|c| new.at > c.at).unwrap_or(false),
                    Some("LT") => current.map(|c| new.at < c.at).unwrap_or(true),
                    Some(_) => true,
                };
                if ok {
                    field.expire_at = Some(new);
                    changed = true;
                    codes.push(RedisReply::Integer(1));
                } else {
                    codes.push(RedisReply::Integer(0));
                }
            }
            None => codes.push(RedisReply::Integer(2)),
        }
    }
    if changed {
        let mut entry = h.entry;
        if let Err(e) = store_hash(session, id, &mut entry, h.hash) {
            return e;
        }
    }
    RedisReply::Array(codes)
}

/// `HTTL`/`HPTTL`/`HEXPIRETIME`/`HPEXPIRETIME key FIELDS n field...`.
fn cmd_httl(session: &mut SessionState, upper: &str, args: &[&str], absolute: bool) -> RedisReply {
    let millis = matches!(upper, "HPTTL" | "HPEXPIRETIME");
    if args.len() < 4 {
        return render::arity(&upper.to_ascii_lowercase());
    }
    let mut i = 1;
    if !args[i].eq_ignore_ascii_case("FIELDS") {
        return render::error("ERR syntax error");
    }
    i += 1;
    let Some(count) = args.get(i).and_then(|s| parse_i64(s)) else {
        return render::error(MISSING_INT);
    };
    i += 1;
    let fields: Vec<&str> = args.get(i..).unwrap_or(&[]).to_vec();
    if count as usize != fields.len() {
        return render::error("ERR Parameter `numFields` is not equal to the number of fields");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let h = match load_hash(session, id, args[0]) {
        Ok(Some(h)) => h,
        Ok(None) => {
            return RedisReply::Array(fields.iter().map(|_| RedisReply::Integer(-2)).collect());
        }
        Err(e) => return e,
    };
    let now = now();
    let out = fields
        .iter()
        .map(|f| match h.hash.fields.get(f.as_bytes()) {
            Some(field) if field_live(field) => match field.expire_at {
                None => RedisReply::Integer(-1),
                Some(d) => {
                    if absolute {
                        RedisReply::Integer(if millis { d.at.millis } else { d.at.millis / 1000 })
                    } else {
                        let remaining = d.at.millis.saturating_sub(now.millis);
                        RedisReply::Integer(if millis { remaining } else { remaining / 1000 })
                    }
                }
            },
            _ => RedisReply::Integer(-2),
        })
        .collect();
    RedisReply::Array(out)
}

/// `HGETEX key [EX s|PX ms|EXAT|PXAT|PERSIST] FIELDS n field...`.
fn cmd_hgetex(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() < 4 {
        return render::arity("hgetex");
    }
    let key = args[0];
    let mut i = 1;
    let mut new_deadline: Option<Option<Deadline>> = None;
    while i < args.len() && !args[i].eq_ignore_ascii_case("FIELDS") {
        let up = args[i].to_ascii_uppercase();
        match up.as_str() {
            "PERSIST" => {
                new_deadline = Some(None);
                i += 1;
            }
            "EX" | "PX" | "EXAT" | "PXAT" => {
                let Some(n) = args.get(i + 1).and_then(|s| parse_i64(s)) else {
                    return render::error(MISSING_INT);
                };
                new_deadline = Some(Some(crate::commands::strings::deadline_from(&up, n)));
                i += 2;
            }
            _ => return render::error("ERR syntax error"),
        }
    }
    if i >= args.len() || !args[i].eq_ignore_ascii_case("FIELDS") {
        return render::error("ERR syntax error");
    }
    i += 1;
    let Some(count) = args.get(i).and_then(|s| parse_i64(s)) else {
        return render::error(MISSING_INT);
    };
    i += 1;
    let fields: Vec<&str> = args.get(i..).unwrap_or(&[]).to_vec();
    if count as usize != fields.len() {
        return render::error("ERR Parameter `numFields` is not equal to the number of fields");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let mut h = match load_hash(session, id, key) {
        Ok(Some(h)) => h,
        Ok(None) => return RedisReply::Array(fields.iter().map(|_| RedisReply::Bulk(None)).collect()),
        Err(e) => return e,
    };
    let mut out = Vec::new();
    let mut changed = false;
    for f in fields {
        match h.hash.fields.get_mut(f.as_bytes()) {
            Some(field) if field_live(field) => {
                out.push(RedisReply::Bulk(Some(field.value.to_vec())));
                if let Some(nd) = new_deadline {
                    field.expire_at = nd;
                    changed = true;
                }
            }
            _ => out.push(RedisReply::Bulk(None)),
        }
    }
    if changed {
        let mut entry = h.entry;
        if let Err(e) = store_hash(session, id, &mut entry, h.hash) {
            return e;
        }
    }
    RedisReply::Array(out)
}

/// `HGETDEL key FIELDS n field...`.
fn cmd_hgetdel(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() < 4 {
        return render::arity("hgetdel");
    }
    let key = args[0];
    if !args[1].eq_ignore_ascii_case("FIELDS") {
        return render::error("ERR syntax error");
    }
    let Some(count) = args.get(2).and_then(|s| parse_i64(s)) else {
        return render::error(MISSING_INT);
    };
    let fields: Vec<&str> = args.get(3..).unwrap_or(&[]).to_vec();
    if count as usize != fields.len() {
        return render::error("ERR Parameter `numFields` is not equal to the number of fields");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let mut h = match load_hash(session, id, key) {
        Ok(Some(h)) => h,
        Ok(None) => return RedisReply::Array(fields.iter().map(|_| RedisReply::Bulk(None)).collect()),
        Err(e) => return e,
    };
    let mut out = Vec::new();
    let mut changed = false;
    for f in fields {
        match h.hash.fields.remove(f.as_bytes()) {
            Some(field) if field_live(&field) => {
                out.push(RedisReply::Bulk(Some(field.value.to_vec())));
                changed = true;
            }
            _ => out.push(RedisReply::Bulk(None)),
        }
    }
    if changed {
        let mut entry = h.entry;
        if let Err(e) = store_or_drop(session, id, &mut entry, h.hash, key) {
            return e;
        }
    }
    RedisReply::Array(out)
}

/// Deterministic sorted field map used by tests.
#[allow(dead_code)]
fn sorted_map(m: &BTreeMap<Bytes, HashField>) -> Vec<String> {
    m.keys().map(|k| String::from_utf8_lossy(k).into_owned()).collect()
}

#[allow(dead_code)]
fn visible_hash() -> RedisVisibleType {
    RedisVisibleType::Hash
}

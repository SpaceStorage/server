//! Redis `strings` + key-expiry family (`017` US1 T026). All commands lower onto
//! the shared key-space entry model (`lower.rs`); the handler keeps no private
//! store. Writes set/clear the entry deadline (data-model §7).

use bytes::Bytes;
use spacestorage_query::LogicalRequest;

use crate::expiry::{Deadline, TtlInstant, report_ttl};
use crate::lower::{EntryPayload, RedisEntry, RedisVisibleType};
use crate::render;
use super::{RedisReply, SessionState};

const MISSING_INT: &str = "ERR value is not an integer or out of range";
const MISSING_FLOAT: &str = "ERR value is not a valid float";

fn bytes_of(entry: &RedisEntry) -> Option<&[u8]> {
    match &entry.payload {
        EntryPayload::Bytes(b) => Some(b),
        _ => None,
    }
}

fn string_entry(value: Vec<u8>) -> RedisEntry {
    RedisEntry::new(Bytes::from_static(b""), EntryPayload::Bytes(value))
}

/// `true` when the current entry is a Redis string *and* a plain byte payload.
fn is_plain_string(entry: &RedisEntry) -> bool {
    entry.visible == RedisVisibleType::String && bytes_of(entry).is_some()
}

fn parse_i64(s: &str) -> Option<i64> {
    s.parse::<i64>().ok()
}

fn parse_f64(s: &str) -> Option<f64> {
    s.parse::<f64>().ok().filter(|f| f.is_finite())
}

/// Cursor into command arguments.
struct Argv<'a> {
    items: &'a [&'a str],
    pos: usize,
}

impl<'a> Argv<'a> {
    fn new(items: &'a [&'a str]) -> Self {
        Self { items, pos: 0 }
    }
    fn next(&mut self) -> Option<&'a str> {
        let v = self.items.get(self.pos).copied();
        if v.is_some() {
            self.pos += 1;
        }
        v
    }
}

/// `SET` flag set (NX/XX/EX/PX/EXAT/PXAT/KEEPTTL).
#[derive(Default)]
struct SetOpts {
    nx: bool,
    xx: bool,
    keep_ttl: bool,
    expire: Option<Deadline>,
}

fn parse_set_opts(argv: &mut Argv) -> Result<SetOpts, RedisReply> {
    let mut opts = SetOpts::default();
    while let Some(raw) = argv.next() {
        let up = raw.to_ascii_uppercase();
        match up.as_str() {
            "NX" => opts.nx = true,
            "XX" => opts.xx = true,
            "KEEPTTL" => opts.keep_ttl = true,
            "GET" => {}
            "EX" | "PX" | "EXAT" | "PXAT" => {
                let Some(n) = argv.next().and_then(parse_i64) else {
                    return Err(render::error("ERR value is not an integer or out of range"));
                };
                opts.expire = Some(deadline_from(&up, n));
            }
            other => {
                return Err(render::error(format!(
                    "ERR syntax error near '{other}'"
                )));
            }
        }
    }
    Ok(opts)
}

pub(crate) fn deadline_from(unit: &str, n: i64) -> Deadline {
    let now = TtlInstant::now().millis;
    match unit {
        "EX" => Deadline::new(TtlInstant::from_unix_millis(now.saturating_add(n * 1000))),
        "PX" => Deadline::new(TtlInstant::from_unix_millis(now.saturating_add(n))),
        "EXAT" => Deadline::new(TtlInstant::from_unix_millis(n * 1000)),
        "PXAT" => Deadline::new(TtlInstant::from_unix_millis(n)),
        _ => Deadline::new(TtlInstant::from_unix_millis(now.saturating_add(n * 1000))),
    }
}

fn expire_opts(arg: &str) -> Option<&str> {
    let up = arg.to_ascii_uppercase();
    Some(match up.as_str() {
        "NX" => "NX",
        "XX" => "XX",
        "GT" => "GT",
        "LT" => "LT",
        _ => return None,
    })
}

/// `EXPIRE`-family conditional update. Returns whether the deadline was applied.
fn apply_expire(
    current: Option<Deadline>,
    _now: TtlInstant,
    new: Deadline,
    cond: Option<&str>,
) -> bool {
    match cond {
        None => true,
        Some("NX") => current.is_none(),
        Some("XX") => current.is_some(),
        Some("GT") => current.map(|c| new.at > c.at).unwrap_or(false),
        Some("LT") => current.map(|c| new.at < c.at).unwrap_or(true),
        Some(_) => true,
    }
}

pub fn dispatch(session: &mut SessionState, upper: &str, args: &[&str]) -> Option<RedisReply> {
    match upper {
        "SET" => Some(cmd_set(session, args)),
        "SETNX" => Some(cmd_setnx(session, args)),
        "SETEX" => Some(cmd_setex(session, args, false)),
        "PSETEX" => Some(cmd_setex(session, args, true)),
        "GET" => Some(cmd_get(session, args)),
        "GETSET" => Some(cmd_getset(session, args)),
        "GETDEL" => Some(cmd_getdel(session, args)),
        "GETEX" => Some(cmd_getex(session, args)),
        "MGET" => Some(cmd_mget(session, args)),
        "MSET" => Some(cmd_mset(session, args)),
        "MSETNX" => Some(cmd_msetnx(session, args)),
        "STRLEN" => Some(cmd_strlen(session, args)),
        "APPEND" => Some(cmd_append(session, args)),
        "GETRANGE" => Some(cmd_getrange(session, args)),
        "SETRANGE" => Some(cmd_setrange(session, args)),
        "INCR" => Some(cmd_incrby(session, args, 1)),
        "DECR" => Some(cmd_incrby(session, args, -1)),
        "INCRBY" => Some(cmd_incrby_arg(session, args, false)),
        "DECRBY" => Some(cmd_incrby_arg(session, args, true)),
        "INCRBYFLOAT" => Some(cmd_incrbyfloat(session, args)),
        "DEL" | "UNLINK" => Some(cmd_del(session, args)),
        "EXISTS" => Some(cmd_exists(session, args)),
        "SCAN" => Some(cmd_scan(session, args)),
        "EXPIRE" => Some(cmd_expire(session, args, ExpUnit::Seconds, false)),
        "PEXPIRE" => Some(cmd_expire(session, args, ExpUnit::Millis, false)),
        "EXPIREAT" => Some(cmd_expire(session, args, ExpUnit::Seconds, true)),
        "PEXPIREAT" => Some(cmd_expire(session, args, ExpUnit::Millis, true)),
        "PERSIST" => Some(cmd_persist(session, args)),
        "TTL" => Some(cmd_ttl(session, args, false)),
        "PTTL" => Some(cmd_ttl(session, args, true)),
        "EXPIRETIME" => Some(cmd_expiretime(session, args, false)),
        "PEXPIRETIME" => Some(cmd_expiretime(session, args, true)),
        _ => None,
    }
}

fn cmd_set(session: &mut SessionState, args: &[&str]) -> RedisReply {
    let mut argv = Argv::new(args);
    let (Some(key), Some(value)) = (argv.next(), argv.next()) else {
        return render::arity("set");
    };
    let limits = session.limit_services.limits();
    if let Err(e) = limits.check_key(key.len() as u64) {
        return render::error(format!("ERR {e}"));
    }
    if let Err(e) = limits.check_value(value.len() as u64) {
        return render::error(format!("ERR {e}"));
    }
    let opts = match parse_set_opts(&mut argv) {
        Ok(o) => o,
        Err(e) => return e,
    };
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let existing = match session.load_entry(id, key) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if opts.nx && existing.is_some() {
        return RedisReply::Bulk(None);
    }
    if opts.xx && existing.is_none() {
        return RedisReply::Bulk(None);
    }
    let mut entry = string_entry(value.as_bytes().to_vec());
    entry.key = Bytes::from(key.as_bytes().to_vec());
    if opts.keep_ttl {
        entry.expire_at = existing.as_ref().and_then(|e| e.expire_at);
    } else {
        entry.expire_at = opts.expire;
    }
    if let Err(e) = session.store_entry(id, &entry) {
        return e;
    }
    RedisReply::Ok
}

fn cmd_setnx(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 2 {
        return render::arity("setnx");
    }
    let (key, value) = (args[0], args[1]);
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    if let Ok(Some(_)) = session.load_entry(id, key) {
        return RedisReply::Integer(0);
    }
    let mut entry = string_entry(value.as_bytes().to_vec());
    entry.key = Bytes::from(key.as_bytes().to_vec());
    match session.store_entry(id, &entry) {
        Ok(()) => RedisReply::Integer(1),
        Err(e) => e,
    }
}

fn cmd_setex(session: &mut SessionState, args: &[&str], millis: bool) -> RedisReply {
    let name = if millis { "psetex" } else { "setex" };
    if args.len() != 3 {
        return render::arity(name);
    }
    let Some(secs) = parse_i64(args[1]) else {
        return render::error(MISSING_INT);
    };
    if secs <= 0 {
        return render::error("ERR invalid expire time in 'setex' command");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let mut entry = string_entry(args[2].as_bytes().to_vec());
    entry.key = Bytes::from(args[0].as_bytes().to_vec());
    let now = TtlInstant::now().millis;
    let at = if millis { now + secs } else { now + secs * 1000 };
    entry.expire_at = Some(Deadline::new(TtlInstant::from_unix_millis(at)));
    match session.store_entry(id, &entry) {
        Ok(()) => RedisReply::Ok,
        Err(e) => e,
    }
}

fn cmd_get(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 1 {
        return render::arity("get");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    match session.load_entry(id, args[0]) {
        Ok(None) => {
            // `017` US4 T055: a miss on a live key-space read is counted.
            session.stats.record_miss();
            RedisReply::Bulk(None)
        }
        Ok(Some(e)) => match bytes_of(&e) {
            Some(b) => {
                let limits = session.limit_services.limits();
                if let Err(err) = limits.check_result(b.len() as u64) {
                    return render::error(format!("ERR {err}"));
                }
                // `017` US4 T055: a hit on a live key-space read is counted.
                session.stats.record_hit();
                RedisReply::Bulk(Some(b.to_vec()))
            }
            None => render::wrongtype(),
        },
        Err(e) => e,
    }
}

fn cmd_getset(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 2 {
        return render::arity("getset");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let old = match session.load_entry(id, args[0]) {
        Ok(Some(e)) => match bytes_of(&e) {
            Some(b) => Some(b.to_vec()),
            None => return render::wrongtype(),
        },
        Ok(None) => None,
        Err(e) => return e,
    };
    let mut entry = string_entry(args[1].as_bytes().to_vec());
    entry.key = Bytes::from(args[0].as_bytes().to_vec());
    if let Err(e) = session.store_entry(id, &entry) {
        return e;
    }
    RedisReply::Bulk(old)
}

fn cmd_getdel(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 1 {
        return render::arity("getdel");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let old = match session.load_entry(id, args[0]) {
        Ok(Some(e)) => match bytes_of(&e) {
            Some(b) => Some(b.to_vec()),
            None => return render::wrongtype(),
        },
        Ok(None) => None,
        Err(e) => return e,
    };
    if old.is_some() {
        if let Err(e) = session.delete_entry(id, args[0]) {
            return e;
        }
    }
    RedisReply::Bulk(old)
}

fn cmd_getex(session: &mut SessionState, args: &[&str]) -> RedisReply {
    let mut argv = Argv::new(args);
    let Some(key) = argv.next() else {
        return render::arity("getex");
    };
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let mut entry = match session.load_entry(id, key) {
        Ok(Some(e)) => e,
        Ok(None) => return RedisReply::Bulk(None),
        Err(e) => return e,
    };
    let Some(value) = bytes_of(&entry).map(|b| b.to_vec()) else {
        return render::wrongtype();
    };
    let mut persist = false;
    let mut new_deadline: Option<Option<Deadline>> = None;
    while let Some(raw) = argv.next() {
        let up = raw.to_ascii_uppercase();
        match up.as_str() {
            "PERSIST" => {
                persist = true;
                new_deadline = Some(None);
            }
            "EX" | "PX" | "EXAT" | "PXAT" => {
                let Some(n) = argv.next().and_then(parse_i64) else {
                    return render::error(MISSING_INT);
                };
                new_deadline = Some(Some(deadline_from(&up, n)));
            }
            other => return render::error(format!("ERR syntax error near '{other}'")),
        }
    }
    if let Some(nd) = new_deadline {
        entry.expire_at = nd;
        if let Err(e) = session.store_entry(id, &entry) {
            return e;
        }
    }
    let _ = persist;
    RedisReply::Bulk(Some(value))
}

fn cmd_mget(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.is_empty() {
        return render::arity("mget");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let mut out = Vec::new();
    for key in args {
        match session.load_entry(id, key) {
            Ok(Some(e)) => match bytes_of(&e) {
                Some(b) => out.push(RedisReply::Bulk(Some(b.to_vec()))),
                None => out.push(RedisReply::Bulk(None)),
            },
            Ok(None) => out.push(RedisReply::Bulk(None)),
            Err(e) => return e,
        }
    }
    RedisReply::Array(out)
}

fn cmd_mset(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.is_empty() || args.len() % 2 != 0 {
        return render::arity("mset");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    for pair in args.chunks(2) {
        let mut entry = string_entry(pair[1].as_bytes().to_vec());
        entry.key = Bytes::from(pair[0].as_bytes().to_vec());
        if let Err(e) = session.store_entry(id, &entry) {
            return e;
        }
    }
    RedisReply::Ok
}

fn cmd_msetnx(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.is_empty() || args.len() % 2 != 0 {
        return render::arity("msetnx");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    for pair in args.chunks(2) {
        match session.load_entry(id, pair[0]) {
            Ok(Some(_)) => return RedisReply::Integer(0),
            Ok(None) => {}
            Err(e) => return e,
        }
    }
    for pair in args.chunks(2) {
        let mut entry = string_entry(pair[1].as_bytes().to_vec());
        entry.key = Bytes::from(pair[0].as_bytes().to_vec());
        if let Err(e) = session.store_entry(id, &entry) {
            return e;
        }
    }
    RedisReply::Integer(1)
}

fn cmd_strlen(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 1 {
        return render::arity("strlen");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    match session.load_entry(id, args[0]) {
        Ok(Some(e)) => match bytes_of(&e) {
            Some(b) => RedisReply::Integer(b.len() as i64),
            None => render::wrongtype(),
        },
        Ok(None) => RedisReply::Integer(0),
        Err(e) => e,
    }
}

fn cmd_append(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 2 {
        return render::arity("append");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let mut value = match session.load_entry(id, args[0]) {
        Ok(Some(e)) => match bytes_of(&e) {
            Some(b) => b.to_vec(),
            None => return render::wrongtype(),
        },
        Ok(None) => Vec::new(),
        Err(e) => return e,
    };
    value.extend_from_slice(args[1].as_bytes());
    let mut entry = string_entry(value);
    entry.key = Bytes::from(args[0].as_bytes().to_vec());
    let len = match bytes_of(&entry) {
        Some(b) => b.len(),
        None => 0,
    };
    match session.store_entry(id, &entry) {
        Ok(()) => RedisReply::Integer(len as i64),
        Err(e) => e,
    }
}

fn norm_range(start: i64, end: i64, len: i64) -> Option<(usize, usize)> {
    if len == 0 {
        return None;
    }
    let mut s = if start < 0 { start + len } else { start };
    let mut e = if end < 0 { end + len } else { end };
    if s < 0 {
        s = 0;
    }
    if e >= len {
        e = len - 1;
    }
    if s > e || s >= len {
        return None;
    }
    Some((s as usize, e as usize))
}

fn cmd_getrange(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 3 {
        return render::arity("getrange");
    }
    let (Some(start), Some(end)) = (parse_i64(args[1]), parse_i64(args[2])) else {
        return render::error(MISSING_INT);
    };
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    match session.load_entry(id, args[0]) {
        Ok(Some(e)) => match bytes_of(&e) {
            Some(b) => match norm_range(start, end, b.len() as i64) {
                Some((s, en)) => RedisReply::Bulk(Some(b[s..=en].to_vec())),
                None => RedisReply::Bulk(Some(Vec::new())),
            },
            None => render::wrongtype(),
        },
        Ok(None) => RedisReply::Bulk(Some(Vec::new())),
        Err(e) => e,
    }
}

fn cmd_setrange(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 3 {
        return render::arity("setrange");
    }
    let Some(offset) = parse_i64(args[1]) else {
        return render::error(MISSING_INT);
    };
    if offset < 0 {
        return render::error("ERR offset is out of range");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let mut value = match session.load_entry(id, args[0]) {
        Ok(Some(e)) => match bytes_of(&e) {
            Some(b) => b.to_vec(),
            None => return render::wrongtype(),
        },
        Ok(None) => Vec::new(),
        Err(e) => return e,
    };
    let off = offset as usize;
    let patch = args[2].as_bytes();
    if !patch.is_empty() {
        // Reject an over-limit resulting length before mutating (015 limit);
        // otherwise `resize` would silently grow the value past `max_value`.
        let new_len = off + patch.len();
        let limits = session.limit_services.limits();
        if let Err(e) = limits.check_value(new_len as u64) {
            return render::limit(&e.to_string());
        }
        if value.len() < new_len {
            value.resize(new_len, 0);
        }
        value[off..off + patch.len()].copy_from_slice(patch);
    }
    let len = value.len();
    let mut entry = string_entry(value);
    entry.key = Bytes::from(args[0].as_bytes().to_vec());
    match session.store_entry(id, &entry) {
        Ok(()) => RedisReply::Integer(len as i64),
        Err(e) => e,
    }
}

fn cmd_incrby(session: &mut SessionState, args: &[&str], delta: i64) -> RedisReply {
    if args.len() != 1 {
        return render::arity("incr");
    }
    incr_common(session, args[0], delta)
}

fn cmd_incrby_arg(session: &mut SessionState, args: &[&str], negate: bool) -> RedisReply {
    if args.len() != 2 {
        return render::arity(if negate { "decrby" } else { "incrby" });
    }
    let Some(n) = parse_i64(args[1]) else {
        return render::error(MISSING_INT);
    };
    incr_common(session, args[0], if negate { -n } else { n })
}

fn incr_common(session: &mut SessionState, key: &str, delta: i64) -> RedisReply {
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let current = match session.load_entry(id, key) {
        Ok(Some(e)) => match bytes_of(&e) {
            Some(b) => match std::str::from_utf8(b).ok().and_then(parse_i64) {
                Some(n) => n,
                None => return render::error(MISSING_INT),
            },
            None => return render::wrongtype(),
        },
        Ok(None) => 0,
        Err(e) => return e,
    };
    let next = match current.checked_add(delta) {
        Some(n) => n,
        None => return render::error("ERR increment or decrement would overflow"),
    };
    let mut entry = string_entry(next.to_string().into_bytes());
    entry.key = Bytes::from(key.as_bytes().to_vec());
    match session.store_entry(id, &entry) {
        Ok(()) => RedisReply::Integer(next),
        Err(e) => e,
    }
}

fn cmd_incrbyfloat(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 2 {
        return render::arity("incrbyfloat");
    }
    let Some(delta) = parse_f64(args[1]) else {
        return render::error(MISSING_FLOAT);
    };
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let current = match session.load_entry(id, args[0]) {
        Ok(Some(e)) => match bytes_of(&e) {
            Some(b) => match std::str::from_utf8(b).ok().and_then(parse_f64) {
                Some(n) => n,
                None => return render::error(MISSING_FLOAT),
            },
            None => return render::wrongtype(),
        },
        Ok(None) => 0.0,
        Err(e) => return e,
    };
    let next = current + delta;
    let text = format_float(next);
    let mut entry = string_entry(text.clone().into_bytes());
    entry.key = Bytes::from(args[0].as_bytes().to_vec());
    match session.store_entry(id, &entry) {
        Ok(()) => RedisReply::Bulk(Some(text.into_bytes())),
        Err(e) => e,
    }
}

fn format_float(f: f64) -> String {
    if f == f.trunc() && f.abs() < 1e17 {
        format!("{}", f as i64)
    } else {
        let s = format!("{f}");
        s
    }
}

fn cmd_del(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.is_empty() {
        return render::arity("del");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let mut n = 0i64;
    for key in args {
        match session.load_entry(id, key) {
            Ok(Some(_)) => match session.delete_entry(id, key) {
                Ok(_) => n += 1,
                Err(e) => return e,
            },
            Ok(None) => {}
            Err(e) => return e,
        }
    }
    RedisReply::Integer(n)
}

fn cmd_exists(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.is_empty() {
        return render::arity("exists");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let mut n = 0i64;
    for key in args {
        match session.load_entry(id, key) {
            Ok(Some(_)) => n += 1,
            Ok(None) => {}
            Err(e) => return e,
        }
    }
    RedisReply::Integer(n)
}

fn cmd_scan(session: &mut SessionState, args: &[&str]) -> RedisReply {
    let cursor: u64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    let mut count = 10usize;
    let mut pattern: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        let opt = args[i].to_ascii_uppercase();
        if opt == "COUNT" {
            if let Some(n) = args.get(i + 1).and_then(|s| s.parse().ok()) {
                count = n;
            }
            i += 2;
        } else if opt == "MATCH" {
            pattern = args.get(i + 1).map(|s| s.to_string());
            i += 2;
        } else {
            i += 1;
        }
    }
    let _id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    match session.planner_exec(LogicalRequest::KeyScan {
        container: session.container.clone(),
        cursor,
        count,
        pattern,
    }) {
        Ok(r) => {
            let keys: Vec<RedisReply> = r
                .rows
                .iter()
                .filter_map(|row| row.first().and_then(|k| k.clone()))
                .map(|k| RedisReply::Bulk(Some(k.into_bytes())))
                .collect();
            RedisReply::Array(vec![
                RedisReply::Bulk(Some(r.tag.into_bytes())),
                RedisReply::Array(keys),
            ])
        }
        Err(e) => e,
    }
}

enum ExpUnit {
    Seconds,
    Millis,
}

fn cmd_expire(
    session: &mut SessionState,
    args: &[&str],
    unit: ExpUnit,
    absolute: bool,
) -> RedisReply {
    if args.len() < 2 {
        return render::arity("expire");
    }
    let Some(n) = parse_i64(args[1]) else {
        return render::error(MISSING_INT);
    };
    let cond = args.get(2).and_then(|a| expire_opts(a));
    if args.len() > 2 && cond.is_none() {
        return render::error("ERR syntax error");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let mut entry = match session.load_entry(id, args[0]) {
        Ok(Some(e)) => e,
        Ok(None) => return RedisReply::Integer(0),
        Err(e) => return e,
    };
    let now = TtlInstant::now();
    let at = match (absolute, &unit) {
        (true, ExpUnit::Seconds) => n.saturating_mul(1000),
        (true, ExpUnit::Millis) => n,
        (false, ExpUnit::Seconds) => now.millis.saturating_add(n.saturating_mul(1000)),
        (false, ExpUnit::Millis) => now.millis.saturating_add(n),
    };
    let new = Deadline::new(TtlInstant::from_unix_millis(at));
    if !apply_expire(entry.expire_at, now, new, cond) {
        return RedisReply::Integer(0);
    }
    entry.expire_at = Some(new);
    match session.store_entry(id, &entry) {
        Ok(()) => RedisReply::Integer(1),
        Err(e) => e,
    }
}

fn cmd_persist(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 1 {
        return render::arity("persist");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let mut entry = match session.load_entry(id, args[0]) {
        Ok(Some(e)) => e,
        Ok(None) => return RedisReply::Integer(0),
        Err(e) => return e,
    };
    if entry.expire_at.is_none() {
        return RedisReply::Integer(0);
    }
    entry.expire_at = None;
    match session.store_entry(id, &entry) {
        Ok(()) => RedisReply::Integer(1),
        Err(e) => e,
    }
}

fn cmd_ttl(session: &mut SessionState, args: &[&str], millis: bool) -> RedisReply {
    let name = if millis { "pttl" } else { "ttl" };
    if args.len() != 1 {
        return render::arity(name);
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    match session.load_entry(id, args[0]) {
        Ok(Some(e)) => RedisReply::Integer(report_ttl(
            true,
            e.expire_at,
            TtlInstant::now(),
            millis,
        )),
        Ok(None) => RedisReply::Integer(-2),
        Err(e) => e,
    }
}

fn cmd_expiretime(session: &mut SessionState, args: &[&str], millis: bool) -> RedisReply {
    let name = if millis { "pexpiretime" } else { "expiretime" };
    if args.len() != 1 {
        return render::arity(name);
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    match session.load_entry(id, args[0]) {
        Ok(Some(e)) => match e.expire_at {
            Some(d) => RedisReply::Integer(if millis { d.at.millis } else { d.at.millis / 1000 }),
            None => RedisReply::Integer(-1),
        },
        Ok(None) => RedisReply::Integer(-2),
        Err(e) => e,
    }
}

/// Test-only: expose the plain-string check.
#[allow(dead_code)]
pub(crate) fn debug_is_plain_string(entry: &RedisEntry) -> bool {
    is_plain_string(entry)
}

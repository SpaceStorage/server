//! Redis `zset` family (`017` US1 T025) incl. the blocking forms (T027).
//! `ZSetValue` keeps member → score; the score-ordered view (numeric, ties broken
//! by member byte order) is derived on demand. A `NaN` score is a command error
//! and never inserts.

use std::collections::BTreeMap;

use bytes::Bytes;

use crate::block::{BlockKind, BlockRequest};
use crate::lower::{EntryPayload, RedisEntry, ZSetValue};
use crate::render;
use super::{RedisReply, SessionState};

type ContainerId = spacestorage_types::ContainerId;

const NOT_FLOAT: &str = "ERR value is not a valid float";

fn fmt_score(f: f64) -> Vec<u8> {
    if f == f.trunc() && f.is_finite() && f.abs() < 1e17 {
        format!("{}", f as i64).into_bytes()
    } else {
        format!("{f}").into_bytes()
    }
}

fn load_zset(
    session: &SessionState,
    id: ContainerId,
    key: &str,
) -> Result<Option<(RedisEntry, ZSetValue)>, RedisReply> {
    match session.load_entry(id, key)? {
        None => Ok(None),
        Some(mut e) => {
            if !matches!(e.payload, EntryPayload::ZSet(_)) {
                return Err(render::wrongtype());
            }
            let z = match std::mem::replace(&mut e.payload, EntryPayload::Bytes(Vec::new())) {
                EntryPayload::ZSet(z) => z,
                _ => unreachable!(),
            };
            Ok(Some((e, z)))
        }
    }
}

fn fresh_zset_entry(key: &str) -> RedisEntry {
    RedisEntry::new(
        Bytes::from(key.as_bytes().to_vec()),
        EntryPayload::ZSet(ZSetValue::default()),
    )
}

fn store_zset(
    session: &SessionState,
    id: ContainerId,
    entry: &mut RedisEntry,
    z: ZSetValue,
    key: &str,
) -> Result<(), RedisReply> {
    let empty = z.by_member.is_empty();
    entry.replace(EntryPayload::ZSet(z));
    if empty {
        session.delete_entry(id, key)?;
        Ok(())
    } else {
        session.store_entry(id, entry)
    }
}

/// Members ordered by (score ascending, member byte order).
fn ordered(z: &ZSetValue) -> Vec<(Bytes, f64)> {
    let mut v: Vec<(Bytes, f64)> = z.by_member.iter().map(|(m, s)| (m.clone(), *s)).collect();
    v.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    v
}

pub fn dispatch(session: &mut SessionState, upper: &str, args: &[&str]) -> Option<RedisReply> {
    match upper {
        "ZADD" => Some(cmd_zadd(session, args)),
        "ZREM" => Some(cmd_zrem(session, args)),
        "ZCARD" => Some(cmd_zcard(session, args)),
        "ZSCORE" => Some(cmd_zscore(session, args)),
        "ZMSCORE" => Some(cmd_zmscore(session, args)),
        "ZCOUNT" => Some(cmd_zcount(session, args)),
        "ZRANK" => Some(cmd_zrank(session, args, false)),
        "ZREVRANK" => Some(cmd_zrank(session, args, true)),
        "ZINCRBY" => Some(cmd_zincrby(session, args)),
        "ZRANGE" => Some(cmd_zrange(session, args)),
        "ZREVRANGE" => Some(cmd_zrevrange(session, args)),
        "ZRANGEBYSCORE" => Some(cmd_zrangebyscore(session, args, false)),
        "ZREVRANGEBYSCORE" => Some(cmd_zrangebyscore(session, args, true)),
        "ZPOPMIN" => Some(cmd_zpop(session, args, false)),
        "ZPOPMAX" => Some(cmd_zpop(session, args, true)),
        "ZREMRANGEBYSCORE" => Some(cmd_zremrangebyscore(session, args)),
        "ZREMRANGEBYRANK" => Some(cmd_zremrangebyrank(session, args)),
        "ZRANGESTORE" => Some(cmd_zrangestore(session, args)),
        "ZUNION" => Some(zsetop_impl(session, args, SetOp::Union, false)),
        "ZINTER" => Some(zsetop_impl(session, args, SetOp::Inter, false)),
        "ZDIFF" => Some(zsetop_impl(session, args, SetOp::Diff, false)),
        "ZUNIONSTORE" => Some(zsetop_impl(session, args, SetOp::Union, true)),
        "ZINTERSTORE" => Some(zsetop_impl(session, args, SetOp::Inter, true)),
        "ZDIFFSTORE" => Some(zsetop_impl(session, args, SetOp::Diff, true)),
        "ZSCAN" => Some(cmd_zscan(session, args)),
        _ => None,
    }
}

fn parse_score(s: &str) -> Option<f64> {
    s.parse::<f64>().ok()
}

fn cmd_zadd(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() < 3 {
        return render::arity("zadd");
    }
    let key = args[0];
    let mut nx = false;
    let mut xx = false;
    let mut gt = false;
    let mut lt = false;
    let mut ch = false;
    let mut incr = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].to_ascii_uppercase().as_str() {
            "NX" => nx = true,
            "XX" => xx = true,
            "GT" => gt = true,
            "LT" => lt = true,
            "CH" => ch = true,
            "INCR" => incr = true,
            _ => break,
        }
        i += 1;
    }
    if nx && (gt || lt) {
        return render::error("ERR GT, LT, and/or NX options at the same time are not compatible");
    }
    let rest = &args[i..];
    if rest.is_empty() || rest.len() % 2 != 0 {
        return render::error("ERR syntax error");
    }
    if incr && rest.len() != 2 {
        return render::error("ERR INCR option supports a single increment-element pair");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (mut entry, mut z) = match load_zset(session, id, key) {
        Ok(Some(v)) => v,
        Ok(None) => (fresh_zset_entry(key), ZSetValue::default()),
        Err(e) => return e,
    };
    let mut added = 0i64;
    let mut changed = 0i64;
    let mut incr_reply: Option<RedisReply> = None;
    for pair in rest.chunks(2) {
        let Some(score) = parse_score(pair[0]) else {
            return render::error(NOT_FLOAT);
        };
        if !score.is_finite() {
            return render::error(NOT_FLOAT);
        }
        let member = Bytes::from(pair[1].as_bytes().to_vec());
        let existing = z.by_member.get(&member).copied();
        let mut new_score = score;
        if incr {
            new_score = existing.unwrap_or(0.0) + score;
            if !new_score.is_finite() {
                return render::error("ERR resulting score is not a number (NaN)");
            }
        }
        if nx && existing.is_some() {
            incr_reply = Some(RedisReply::Bulk(None));
            continue;
        }
        if xx && existing.is_none() {
            incr_reply = Some(RedisReply::Bulk(None));
            continue;
        }
        if gt && existing.map(|e| new_score <= e).unwrap_or(false) {
            incr_reply = Some(RedisReply::Bulk(None));
            continue;
        }
        if lt && existing.map(|e| new_score >= e).unwrap_or(false) {
            incr_reply = Some(RedisReply::Bulk(None));
            continue;
        }
        match existing {
            None => {
                added += 1;
                changed += 1;
                z.by_member.insert(member, new_score);
            }
            Some(e) if e != new_score => {
                changed += 1;
                z.by_member.insert(member, new_score);
            }
            Some(_) => {}
        }
        if incr {
            incr_reply = Some(RedisReply::Bulk(Some(fmt_score(new_score))));
        }
    }
    if let Err(e) = store_zset(session, id, &mut entry, z, key) {
        return e;
    }
    if incr {
        return incr_reply.unwrap_or(RedisReply::Bulk(None));
    }
    RedisReply::Integer(if ch { changed } else { added })
}

fn cmd_zrem(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() < 2 {
        return render::arity("zrem");
    }
    let key = args[0];
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (mut entry, mut z) = match load_zset(session, id, key) {
        Ok(Some(v)) => v,
        Ok(None) => return RedisReply::Integer(0),
        Err(e) => return e,
    };
    let mut n = 0i64;
    for m in &args[1..] {
        if z.by_member.remove(m.as_bytes()).is_some() {
            n += 1;
        }
    }
    if n > 0 {
        if let Err(e) = store_zset(session, id, &mut entry, z, key) {
            return e;
        }
    }
    RedisReply::Integer(n)
}

fn cmd_zcard(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 1 {
        return render::arity("zcard");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    match load_zset(session, id, args[0]) {
        Ok(Some((_, z))) => RedisReply::Integer(z.by_member.len() as i64),
        Ok(None) => RedisReply::Integer(0),
        Err(e) => e,
    }
}

fn cmd_zscore(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 2 {
        return render::arity("zscore");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    match load_zset(session, id, args[0]) {
        Ok(Some((_, z))) => match z.by_member.get(args[1].as_bytes()) {
            Some(s) => RedisReply::Bulk(Some(fmt_score(*s))),
            None => RedisReply::Bulk(None),
        },
        Ok(None) => RedisReply::Bulk(None),
        Err(e) => e,
    }
}

fn cmd_zmscore(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() < 2 {
        return render::arity("zmscore");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let z = match load_zset(session, id, args[0]) {
        Ok(Some((_, z))) => Some(z),
        Ok(None) => None,
        Err(e) => return e,
    };
    let out = args[1..]
        .iter()
        .map(|m| match z.as_ref().and_then(|z| z.by_member.get(m.as_bytes())) {
            Some(s) => RedisReply::Bulk(Some(fmt_score(*s))),
            None => RedisReply::Bulk(None),
        })
        .collect();
    RedisReply::Array(out)
}

#[derive(Clone, Copy)]
struct ScoreBound {
    value: f64,
    inclusive: bool,
}

fn parse_score_bound(s: &str) -> Option<ScoreBound> {
    let (inclusive, rest) = match s.strip_prefix('(') {
        Some(r) => (false, r),
        None => (true, s),
    };
    let value = match rest.to_ascii_lowercase().as_str() {
        "-inf" => f64::NEG_INFINITY,
        "+inf" | "inf" => f64::INFINITY,
        _ => rest.parse::<f64>().ok()?,
    };
    Some(ScoreBound { value, inclusive })
}

fn score_in(score: f64, lo: ScoreBound, hi: ScoreBound) -> bool {
    let lo_ok = if lo.inclusive { score >= lo.value } else { score > lo.value };
    let hi_ok = if hi.inclusive { score <= hi.value } else { score < hi.value };
    lo_ok && hi_ok
}

fn cmd_zcount(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 3 {
        return render::arity("zcount");
    }
    let (Some(lo), Some(hi)) = (parse_score_bound(args[1]), parse_score_bound(args[2])) else {
        return render::error("ERR min or max is not a float");
    };
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    match load_zset(session, id, args[0]) {
        Ok(Some((_, z))) => {
            let n = z.by_member.values().filter(|s| score_in(**s, lo, hi)).count() as i64;
            RedisReply::Integer(n)
        }
        Ok(None) => RedisReply::Integer(0),
        Err(e) => e,
    }
}

fn cmd_zrank(session: &mut SessionState, args: &[&str], rev: bool) -> RedisReply {
    if args.len() != 2 {
        return render::arity(if rev { "zrevrank" } else { "zrank" });
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    match load_zset(session, id, args[0]) {
        Ok(Some((_, z))) => {
            let order = ordered(&z);
            let pos = order.iter().position(|(m, _)| m.as_ref() == args[1].as_bytes());
            match pos {
                Some(i) => RedisReply::Integer(if rev {
                    (order.len() - 1 - i) as i64
                } else {
                    i as i64
                }),
                None => RedisReply::Bulk(None),
            }
        }
        Ok(None) => RedisReply::Bulk(None),
        Err(e) => e,
    }
}

fn cmd_zincrby(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 3 {
        return render::arity("zincrby");
    }
    let Some(delta) = parse_score(args[1]) else {
        return render::error(NOT_FLOAT);
    };
    if !delta.is_finite() {
        return render::error(NOT_FLOAT);
    }
    let key = args[0];
    let member = Bytes::from(args[2].as_bytes().to_vec());
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (mut entry, mut z) = match load_zset(session, id, key) {
        Ok(Some(v)) => v,
        Ok(None) => (fresh_zset_entry(key), ZSetValue::default()),
        Err(e) => return e,
    };
    let next = z.by_member.get(&member).copied().unwrap_or(0.0) + delta;
    if !next.is_finite() {
        return render::error("ERR resulting score is not a number (NaN)");
    }
    z.by_member.insert(member, next);
    if let Err(e) = store_zset(session, id, &mut entry, z, key) {
        return e;
    }
    RedisReply::Bulk(Some(fmt_score(next)))
}

enum RangeKind {
    Rank,
    Score,
    Lex,
}

enum LexBound {
    NegInf,
    PosInf,
    Incl(Bytes),
    Excl(Bytes),
}

fn parse_lex(s: &str) -> Option<LexBound> {
    match s {
        "-" => Some(LexBound::NegInf),
        "+" => Some(LexBound::PosInf),
        _ if s.starts_with('[') => Some(LexBound::Incl(Bytes::copy_from_slice(&s.as_bytes()[1..]))),
        _ if s.starts_with('(') => Some(LexBound::Excl(Bytes::copy_from_slice(&s.as_bytes()[1..]))),
        _ => None,
    }
}

fn lex_in(m: &Bytes, lo: &LexBound, hi: &LexBound) -> bool {
    let lo_ok = match lo {
        LexBound::NegInf => true,
        LexBound::PosInf => false,
        LexBound::Incl(b) => m >= b,
        LexBound::Excl(b) => m > b,
    };
    let hi_ok = match hi {
        LexBound::PosInf => true,
        LexBound::NegInf => false,
        LexBound::Incl(b) => m <= b,
        LexBound::Excl(b) => m < b,
    };
    lo_ok && hi_ok
}

/// The core `ZRANGE`-family selector.
fn select_range(
    z: &ZSetValue,
    kind: RangeKind,
    rev: bool,
    a: &str,
    b: &str,
    limit: Option<(i64, i64)>,
) -> Result<Vec<(Bytes, f64)>, RedisReply> {
    let order = ordered(z);
    let mut out: Vec<(Bytes, f64)> = match kind {
        RangeKind::Rank => {
            let start = a.parse::<i64>().map_err(|_| render::error("ERR value is not an integer or out of range"))?;
            let stop = b.parse::<i64>().map_err(|_| render::error("ERR value is not an integer or out of range"))?;
            let mut ranked = if rev {
                let mut r = order.clone();
                r.reverse();
                r
            } else {
                order.clone()
            };
            match super::list::norm_for_rank(ranked.len() as i64, start, stop) {
                Some((s, e)) => {
                    let slice: Vec<_> = ranked.drain(s..=e).collect();
                    slice
                }
                None => Vec::new(),
            }
        }
        RangeKind::Score => {
            let (b1, b2) = (
                parse_score_bound(a).ok_or_else(|| render::error("ERR min or max is not a float"))?,
                parse_score_bound(b).ok_or_else(|| render::error("ERR min or max is not a float"))?,
            );
            let (lo, hi) = if rev { (b2, b1) } else { (b1, b2) };
            let mut filtered: Vec<_> = out_iter(&order, |s, _| score_in(s, lo, hi));
            if rev {
                filtered.reverse();
            }
            filtered
        }
        RangeKind::Lex => {
            let (l1, l2) = (
                parse_lex(a).ok_or_else(|| render::error("ERR min or max not valid string range item"))?,
                parse_lex(b).ok_or_else(|| render::error("ERR min or max not valid string range item"))?,
            );
            let (lo, hi) = if rev { (l2, l1) } else { (l1, l2) };
            let mut filtered: Vec<_> = out_iter(&order, |_, m| lex_in(m, &lo, &hi));
            if rev {
                filtered.reverse();
            }
            filtered
        }
    };
    if let Some((offset, count)) = limit {
        let off = offset.max(0) as usize;
        if off >= out.len() {
            out.clear();
        } else {
            let take = if count < 0 { out.len() - off } else { count as usize };
            out = out.into_iter().skip(off).take(take).collect();
        }
    }
    Ok(out)
}

fn out_iter<F: Fn(f64, &Bytes) -> bool>(order: &[(Bytes, f64)], f: F) -> Vec<(Bytes, f64)> {
    order
        .iter()
        .filter(|(m, s)| f(*s, m))
        .cloned()
        .collect()
}

/// Parse trailing `[WITHSCORES] [LIMIT o c]` style options for range commands.
struct RangeOpts {
    kind: RangeKind,
    rev: bool,
    withscores: bool,
    limit: Option<(i64, i64)>,
}

fn parse_range_opts(args: &[&str]) -> Result<RangeOpts, RedisReply> {
    let mut kind = RangeKind::Rank;
    let mut rev = false;
    let mut withscores = false;
    let mut limit = None;
    let mut i = 3;
    while i < args.len() {
        match args[i].to_ascii_uppercase().as_str() {
            "BYSCORE" => kind = RangeKind::Score,
            "BYLEX" => kind = RangeKind::Lex,
            "REV" => rev = true,
            "WITHSCORES" => withscores = true,
            "LIMIT" => {
                let (Some(o), Some(c)) = (
                    args.get(i + 1).and_then(|s| s.parse::<i64>().ok()),
                    args.get(i + 2).and_then(|s| s.parse::<i64>().ok()),
                ) else {
                    return Err(render::error("ERR syntax error"));
                };
                limit = Some((o, c));
                i += 2;
            }
            _ => return Err(render::error("ERR syntax error")),
        }
        i += 1;
    }
    Ok(RangeOpts {
        kind,
        rev,
        withscores,
        limit,
    })
}

fn render_scored(items: Vec<(Bytes, f64)>, withscores: bool) -> RedisReply {
    let mut out = Vec::new();
    for (m, s) in items {
        out.push(RedisReply::Bulk(Some(m.to_vec())));
        if withscores {
            out.push(RedisReply::Bulk(Some(fmt_score(s))));
        }
    }
    RedisReply::Array(out)
}

fn cmd_zrange(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() < 3 {
        return render::arity("zrange");
    }
    let opts = match parse_range_opts(args) {
        Ok(o) => o,
        Err(e) => return e,
    };
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let z = match load_zset(session, id, args[0]) {
        Ok(Some((_, z))) => z,
        Ok(None) => return RedisReply::Array(vec![]),
        Err(e) => return e,
    };
    match select_range(&z, opts.kind, opts.rev, args[1], args[2], opts.limit) {
        Ok(items) => render_scored(items, opts.withscores),
        Err(e) => e,
    }
}

fn cmd_zrevrange(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() < 3 {
        return render::arity("zrevrange");
    }
    let withscores = args[3..].iter().any(|a| a.eq_ignore_ascii_case("WITHSCORES"));
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let z = match load_zset(session, id, args[0]) {
        Ok(Some((_, z))) => z,
        Ok(None) => return RedisReply::Array(vec![]),
        Err(e) => return e,
    };
    match select_range(&z, RangeKind::Rank, true, args[1], args[2], None) {
        Ok(items) => render_scored(items, withscores),
        Err(e) => e,
    }
}

fn cmd_zrangebyscore(session: &mut SessionState, args: &[&str], rev: bool) -> RedisReply {
    let name = if rev { "zrevrangebyscore" } else { "zrangebyscore" };
    if args.len() < 3 {
        return render::arity(name);
    }
    let mut withscores = false;
    let mut limit = None;
    let mut i = 3;
    while i < args.len() {
        match args[i].to_ascii_uppercase().as_str() {
            "WITHSCORES" => withscores = true,
            "LIMIT" => {
                let (Some(o), Some(c)) = (
                    args.get(i + 1).and_then(|s| s.parse::<i64>().ok()),
                    args.get(i + 2).and_then(|s| s.parse::<i64>().ok()),
                ) else {
                    return render::error("ERR syntax error");
                };
                limit = Some((o, c));
                i += 2;
            }
            _ => return render::error("ERR syntax error"),
        }
        i += 1;
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let z = match load_zset(session, id, args[0]) {
        Ok(Some((_, z))) => z,
        Ok(None) => return RedisReply::Array(vec![]),
        Err(e) => return e,
    };
    match select_range(&z, RangeKind::Score, rev, args[1], args[2], limit) {
        Ok(items) => render_scored(items, withscores),
        Err(e) => e,
    }
}

fn cmd_zpop(session: &mut SessionState, args: &[&str], max: bool) -> RedisReply {
    let name = if max { "zpopmax" } else { "zpopmin" };
    if args.is_empty() {
        return render::arity(name);
    }
    let count = match args.get(1) {
        None => 1i64,
        Some(s) => match s.parse::<i64>() {
            Ok(n) => n,
            Err(_) => return render::error("ERR value is not an integer or out of range"),
        },
    };
    let key = args[0];
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (mut entry, mut z) = match load_zset(session, id, key) {
        Ok(Some(v)) => v,
        Ok(None) => return RedisReply::Array(vec![]),
        Err(e) => return e,
    };
    let mut order = ordered(&z);
    if max {
        order.reverse();
    }
    let n = count.max(0) as usize;
    let mut out = Vec::new();
    for (m, s) in order.into_iter().take(n) {
        z.by_member.remove(&m);
        out.push(RedisReply::Bulk(Some(m.to_vec())));
        out.push(RedisReply::Bulk(Some(fmt_score(s))));
    }
    if let Err(e) = store_zset(session, id, &mut entry, z, key) {
        return e;
    }
    RedisReply::Array(out)
}

fn cmd_zremrangebyscore(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 3 {
        return render::arity("zremrangebyscore");
    }
    let (Some(lo), Some(hi)) = (parse_score_bound(args[1]), parse_score_bound(args[2])) else {
        return render::error("ERR min or max is not a float");
    };
    let key = args[0];
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (mut entry, mut z) = match load_zset(session, id, key) {
        Ok(Some(v)) => v,
        Ok(None) => return RedisReply::Integer(0),
        Err(e) => return e,
    };
    let doomed: Vec<Bytes> = z
        .by_member
        .iter()
        .filter(|(_, s)| score_in(**s, lo, hi))
        .map(|(m, _)| m.clone())
        .collect();
    let n = doomed.len() as i64;
    for m in doomed {
        z.by_member.remove(&m);
    }
    if n > 0 {
        if let Err(e) = store_zset(session, id, &mut entry, z, key) {
            return e;
        }
    }
    RedisReply::Integer(n)
}

fn cmd_zremrangebyrank(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 3 {
        return render::arity("zremrangebyrank");
    }
    let (Some(start), Some(stop)) = (args[1].parse::<i64>().ok(), args[2].parse::<i64>().ok()) else {
        return render::error("ERR value is not an integer or out of range");
    };
    let key = args[0];
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (mut entry, mut z) = match load_zset(session, id, key) {
        Ok(Some(v)) => v,
        Ok(None) => return RedisReply::Integer(0),
        Err(e) => return e,
    };
    let order = ordered(&z);
    let doomed: Vec<Bytes> = match super::list::norm_for_rank(order.len() as i64, start, stop) {
        Some((s, e)) => order[s..=e].iter().map(|(m, _)| m.clone()).collect(),
        None => Vec::new(),
    };
    let n = doomed.len() as i64;
    for m in doomed {
        z.by_member.remove(&m);
    }
    if n > 0 {
        if let Err(e) = store_zset(session, id, &mut entry, z, key) {
            return e;
        }
    }
    RedisReply::Integer(n)
}

fn cmd_zrangestore(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() < 4 {
        return render::arity("zrangestore");
    }
    let dest = args[0];
    let mut kind = RangeKind::Rank;
    let mut rev = false;
    let mut limit = None;
    let mut i = 4;
    while i < args.len() {
        match args[i].to_ascii_uppercase().as_str() {
            "BYSCORE" => kind = RangeKind::Score,
            "BYLEX" => kind = RangeKind::Lex,
            "REV" => rev = true,
            "LIMIT" => {
                let (Some(o), Some(c)) = (
                    args.get(i + 1).and_then(|s| s.parse::<i64>().ok()),
                    args.get(i + 2).and_then(|s| s.parse::<i64>().ok()),
                ) else {
                    return render::error("ERR syntax error");
                };
                limit = Some((o, c));
                i += 2;
            }
            _ => return render::error("ERR syntax error"),
        }
        i += 1;
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let z = match load_zset(session, id, args[1]) {
        Ok(Some((_, z))) => z,
        Ok(None) => ZSetValue::default(),
        Err(e) => return e,
    };
    let items = match select_range(&z, kind, rev, args[2], args[3], limit) {
        Ok(items) => items,
        Err(e) => return e,
    };
    let n = items.len() as i64;
    let mut result = ZSetValue::default();
    for (m, s) in items {
        result.by_member.insert(m, s);
    }
    let mut entry = fresh_zset_entry(dest);
    if let Err(e) = store_zset(session, id, &mut entry, result, dest) {
        return e;
    }
    RedisReply::Integer(n)
}

enum SetOp {
    Union,
    Inter,
    Diff,
}

#[derive(Clone, Copy)]
enum Aggregate {
    Sum,
    Min,
    Max,
}

fn zsetop_impl(session: &mut SessionState, args: &[&str], op: SetOp, store: bool) -> RedisReply {
    let name = match op {
        SetOp::Union => "zunion",
        SetOp::Inter => "zinter",
        SetOp::Diff => "zdiff",
    };
    if args.is_empty() {
        return render::arity(name);
    }
    let dest = if store { Some(args[0]) } else { None };
    let base = if store { 1 } else { 0 };
    let Some(num) = args.get(base).and_then(|s| s.parse::<usize>().ok()) else {
        return render::error("ERR value is not an integer or out of range");
    };
    let key_start = base + 1;
    if num == 0 || args.len() < key_start + num {
        return render::error("ERR at least 1 input key is needed for ZUNIONSTORE/ZINTERSTORE");
    }
    let keys: Vec<&str> = args[key_start..key_start + num].to_vec();
    let mut weights = vec![1.0f64; num];
    let mut aggregate = Aggregate::Sum;
    let mut withscores = false;
    let mut i = key_start + num;
    while i < args.len() {
        match args[i].to_ascii_uppercase().as_str() {
            "WEIGHTS" => {
                for (wi, w) in args[i + 1..].iter().take(num).enumerate() {
                    match w.parse::<f64>() {
                        Ok(v) => weights[wi] = v,
                        Err(_) => return render::error("ERR weight value is not a float"),
                    }
                }
                i += 1 + num;
            }
            "AGGREGATE" => {
                match args.get(i + 1).map(|s| s.to_ascii_uppercase()) {
                    Some(s) if s == "SUM" => aggregate = Aggregate::Sum,
                    Some(s) if s == "MIN" => aggregate = Aggregate::Min,
                    Some(s) if s == "MAX" => aggregate = Aggregate::Max,
                    _ => return render::error("ERR syntax error"),
                }
                i += 2;
            }
            "WITHSCORES" => {
                withscores = true;
                i += 1;
            }
            _ => return render::error("ERR syntax error"),
        }
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let mut sets: Vec<ZSetValue> = Vec::new();
    for k in &keys {
        match load_zset(session, id, k) {
            Ok(Some((_, z))) => sets.push(z),
            Ok(None) => sets.push(ZSetValue::default()),
            Err(e) => return e,
        }
    }
    let mut acc: BTreeMap<Bytes, f64> = BTreeMap::new();
    match op {
        SetOp::Union => {
            for (si, z) in sets.iter().enumerate() {
                for (m, s) in &z.by_member {
                    let weighted = s * weights[si];
                    acc.entry(m.clone())
                        .and_modify(|v| {
                            *v = match aggregate {
                                Aggregate::Sum => *v + weighted,
                                Aggregate::Min => v.min(weighted),
                                Aggregate::Max => v.max(weighted),
                            }
                        })
                        .or_insert(weighted);
                }
            }
        }
        SetOp::Inter => {
            if let Some(first) = sets.first() {
                for (m, s) in &first.by_member {
                    let mut value = s * weights[0];
                    let mut present = true;
                    for (si, z) in sets.iter().enumerate().skip(1) {
                        match z.by_member.get(m) {
                            Some(other) => {
                                let weighted = other * weights[si];
                                value = match aggregate {
                                    Aggregate::Sum => value + weighted,
                                    Aggregate::Min => value.min(weighted),
                                    Aggregate::Max => value.max(weighted),
                                };
                            }
                            None => {
                                present = false;
                                break;
                            }
                        }
                    }
                    if present {
                        acc.insert(m.clone(), value);
                    }
                }
            }
        }
        SetOp::Diff => {
            if let Some(first) = sets.first() {
                for (m, s) in &first.by_member {
                    let in_others = sets.iter().skip(1).any(|z| z.by_member.contains_key(m));
                    if !in_others {
                        acc.insert(m.clone(), *s);
                    }
                }
            }
        }
    }
    if store {
        let dest = dest.unwrap();
        let n = acc.len() as i64;
        let mut entry = fresh_zset_entry(dest);
        let value = ZSetValue { by_member: acc };
        if let Err(e) = store_zset(session, id, &mut entry, value, dest) {
            return e;
        }
        RedisReply::Integer(n)
    } else {
        let mut items: Vec<(Bytes, f64)> = acc.into_iter().collect();
        items.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        render_scored(items, withscores)
    }
}

fn cmd_zscan(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() < 2 {
        return render::arity("zscan");
    }
    let mut i = 2;
    let mut count = 10usize;
    let mut pattern: Option<String> = None;
    while i < args.len() {
        match args[i].to_ascii_uppercase().as_str() {
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
            _ => i += 1,
        }
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let z = match load_zset(session, id, args[0]) {
        Ok(Some((_, z))) => z,
        Ok(None) => ZSetValue::default(),
        Err(e) => return e,
    };
    let items: Vec<RedisReply> = ordered(&z)
        .into_iter()
        .filter(|(m, _)| {
            pattern
                .as_deref()
                .map(|p| crate::lower::glob_match(p, &String::from_utf8_lossy(m)))
                .unwrap_or(true)
        })
        .take(count.max(1))
        .flat_map(|(m, s)| {
            vec![
                RedisReply::Bulk(Some(m.to_vec())),
                RedisReply::Bulk(Some(fmt_score(s))),
            ]
        })
        .collect();
    RedisReply::Array(vec![
        RedisReply::Bulk(Some(b"0".to_vec())),
        RedisReply::Array(items),
    ])
}

/// `017` T027: immediate attempt for `BZPOPMIN`/`BZPOPMAX`.
pub fn try_block_pop(session: &mut SessionState, req: &BlockRequest) -> Option<RedisReply> {
    let max = matches!(req.kind, BlockKind::BzPopMax);
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return Some(e),
    };
    for k in &req.keys {
        let key = String::from_utf8_lossy(k).into_owned();
        let (mut entry, mut z) = match load_zset(session, id, &key) {
            Ok(Some(v)) => v,
            Ok(None) => continue,
            Err(e) => return Some(e),
        };
        let mut order = ordered(&z);
        if max {
            order.reverse();
        }
        let Some((member, score)) = order.into_iter().next() else {
            continue;
        };
        z.by_member.remove(&member);
        if let Err(e) = store_zset(session, id, &mut entry, z, &key) {
            return Some(e);
        }
        return Some(RedisReply::Array(vec![
            RedisReply::Bulk(Some(k.to_vec())),
            RedisReply::Bulk(Some(member.to_vec())),
            RedisReply::Bulk(Some(fmt_score(score))),
        ]));
    }
    None
}

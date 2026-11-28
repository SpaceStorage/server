//! Redis `stream` family + consumer groups (`017` US2 T034–T041).
//!
//! Streams are typed entries inside the namespace key-space container; the
//! handler keeps no private store. Total-order streams (`Total`, the default)
//! present a `006` leadership-lease epoch on every append (research R6,
//! constitution XII); producer-order streams take no lease. `XAUTOCLAIM` is on
//! the not-supported list and is rejected before this struct is touched.

use std::collections::BTreeMap;

use bytes::Bytes;

use crate::block::{BlockKind, BlockRequest, XReadSpec};
use crate::expiry::now_unix_millis;
use crate::lower::{
    ConsumerGroup, EntryPayload, PendingEntry, RedisEntry, StreamEntry, StreamId, StreamOrder,
    StreamValue,
};
use crate::render;
use super::{RedisReply, SessionState};

type ContainerId = spacestorage_types::ContainerId;

const INVALID_ID: &str = "ERR Invalid stream ID specified as stream command argument";
const SMALL_ID: &str =
    "ERR The ID specified in XADD is equal or smaller than the target stream top item";

fn millis_now() -> u64 {
    now_unix_millis().max(0) as u64
}

fn fmt_id(id: StreamId) -> String {
    format!("{}-{}", id.ms, id.seq)
}

fn id_bulk(id: StreamId) -> RedisReply {
    RedisReply::Bulk(Some(fmt_id(id).into_bytes()))
}

fn no_group(group: &str, key: &str) -> RedisReply {
    RedisReply::Error(format!(
        "NOGROUP No such consumer group '{group}' for key name '{key}'"
    ))
}

/// Total-order lease refusal (data-model: `-ERR` unavailable, command name).
fn lease_error(cmd: &str, e: crate::lease::LeaseError) -> RedisReply {
    RedisReply::Error(format!("ERR {e} for command '{cmd}'"))
}

const MIN_ID: StreamId = StreamId { ms: 0, seq: 0 };
const MAX_ID: StreamId = StreamId {
    ms: u64::MAX,
    seq: u64::MAX,
};

// ---------------------------------------------------------------------------
// Load / store
// ---------------------------------------------------------------------------

fn load_stream(
    session: &SessionState,
    id: ContainerId,
    key: &str,
) -> Result<Option<(RedisEntry, StreamValue)>, RedisReply> {
    match session.load_entry(id, key)? {
        None => Ok(None),
        Some(mut e) => {
            if !matches!(e.payload, EntryPayload::Stream(_)) {
                return Err(render::wrongtype());
            }
            let s = match std::mem::replace(&mut e.payload, EntryPayload::Bytes(Vec::new())) {
                EntryPayload::Stream(s) => s,
                _ => unreachable!(),
            };
            Ok(Some((e, s)))
        }
    }
}

fn fresh_stream_entry(key: &str, order: StreamOrder, lease: Option<u64>) -> RedisEntry {
    let s = match order {
        StreamOrder::Total => StreamValue::new_total(lease.unwrap_or(1)),
        StreamOrder::Producer => StreamValue::new_producer(),
    };
    RedisEntry::new(Bytes::from(key.as_bytes().to_vec()), EntryPayload::Stream(s))
}

/// Streams are never dropped when empty (unlike lists/sets): always store.
fn store_stream(
    session: &SessionState,
    id: ContainerId,
    entry: &mut RedisEntry,
    s: StreamValue,
) -> Result<(), RedisReply> {
    entry.replace(EntryPayload::Stream(s));
    session.store_entry(id, entry)
}

/// Remove the PEL record for a deleted/trimmed id from every group (Redis 7.4
/// trim rules: pending records for deleted ids are removed).
fn drop_pending(s: &mut StreamValue, id: StreamId) {
    for g in s.groups.values_mut() {
        g.pending.remove(&id);
    }
}

// ---------------------------------------------------------------------------
// ID parsing / generation
// ---------------------------------------------------------------------------

enum AddId {
    /// `*`
    Auto,
    /// `<ms>-*`
    AutoSeq { ms: u64 },
    /// `<ms>-<seq>` (incl. bare `<ms>` = `<ms>-0`)
    Explicit(StreamId),
}

fn parse_u64(s: &str) -> Option<u64> {
    if s.is_empty() {
        None
    } else {
        s.parse::<u64>().ok()
    }
}

fn parse_add_id(tok: &str) -> Option<AddId> {
    if tok == "*" {
        return Some(AddId::Auto);
    }
    if let Some((ms, seq)) = tok.split_once('-') {
        let ms = parse_u64(ms)?;
        if seq == "*" {
            return Some(AddId::AutoSeq { ms });
        }
        return Some(AddId::Explicit(StreamId {
            ms,
            seq: parse_u64(seq)?,
        }));
    }
    Some(AddId::Explicit(StreamId { ms: parse_u64(tok)?, seq: 0 }))
}

/// Generate the id for an `XADD` given the stream's last id.
fn generate_id(add: AddId, last: StreamId) -> Result<StreamId, String> {
    match add {
        AddId::Auto => {
            let now = millis_now();
            if now > last.ms {
                Ok(StreamId { ms: now, seq: 0 })
            } else if last.seq == u64::MAX {
                Err(SMALL_ID.to_string())
            } else {
                Ok(StreamId { ms: last.ms, seq: last.seq + 1 })
            }
        }
        AddId::AutoSeq { ms } => {
            if ms > last.ms {
                Ok(StreamId { ms, seq: 0 })
            } else if ms == last.ms {
                if last.seq == u64::MAX {
                    Err(SMALL_ID.to_string())
                } else {
                    Ok(StreamId { ms, seq: last.seq + 1 })
                }
            } else {
                Err(SMALL_ID.to_string())
            }
        }
        AddId::Explicit(id) => {
            if id > last {
                Ok(id)
            } else {
                Err(SMALL_ID.to_string())
            }
        }
    }
}

/// Parse an explicit id for `XRANGE`/`XREAD`/`XGROUP` bounds (`-`/`+` alone are
/// caller-handled). Returns `(id, exclusive)`.
fn parse_range_id(tok: &str, is_start: bool) -> Option<(StreamId, bool)> {
    let (excl, body) = match tok.strip_prefix('(') {
        Some(b) => (true, b),
        None => (false, tok),
    };
    let id = if let Some((ms, seq)) = body.split_once('-') {
        StreamId {
            ms: parse_u64(ms)?,
            seq: parse_u64(seq)?,
        }
    } else {
        let ms = parse_u64(body)?;
        StreamId {
            ms,
            seq: if is_start { 0 } else { u64::MAX },
        }
    };
    Some((id, excl))
}

/// Parse a plain id token (`$` handled by callers) used by `XREAD`/`XGROUP`.
fn parse_simple_id(tok: &str) -> Option<StreamId> {
    if let Some((ms, seq)) = tok.split_once('-') {
        Some(StreamId {
            ms: parse_u64(ms)?,
            seq: parse_u64(seq)?,
        })
    } else {
        Some(StreamId { ms: parse_u64(tok)?, seq: 0 })
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn entry_reply(e: &StreamEntry) -> RedisReply {
    let mut fields = Vec::with_capacity(e.fields.len() * 2);
    for (f, v) in &e.fields {
        fields.push(RedisReply::Bulk(Some(f.to_vec())));
        fields.push(RedisReply::Bulk(Some(v.to_vec())));
    }
    RedisReply::Array(vec![id_bulk(e.id), RedisReply::Array(fields)])
}

fn fields_from(args: &[&str]) -> Vec<(Bytes, Bytes)> {
    args.chunks(2)
        .map(|p| {
            (
                Bytes::from(p[0].as_bytes().to_vec()),
                Bytes::from(p.get(1).map(|s| s.as_bytes()).unwrap_or_default().to_vec()),
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Trim
// ---------------------------------------------------------------------------

enum TrimKind {
    MaxLen(u64),
    MinId(StreamId),
}

struct TrimSpec {
    kind: TrimKind,
}

/// Parse `MAXLEN|MINID [~|=] <threshold> [LIMIT n]` starting at `args[i]`.
/// Returns the spec and the index just past it.
fn parse_trim(args: &[&str], i: usize) -> Option<(TrimSpec, usize)> {
    let which = args.get(i)?.to_ascii_uppercase();
    let mut j = i + 1;
    // optional `~` / `=`
    if matches!(args.get(j), Some(t) if *t == "~" || *t == "=") {
        j += 1;
    }
    let kind = match which.as_str() {
        "MAXLEN" => TrimKind::MaxLen(parse_u64(args.get(j)?)?),
        "MINID" => TrimKind::MinId(parse_simple_id(args.get(j)?)?),
        _ => return None,
    };
    j += 1;
    if matches!(args.get(j), Some(t) if t.eq_ignore_ascii_case("LIMIT")) {
        j += 2;
    }
    Some((TrimSpec { kind }, j))
}

/// Drop the oldest entries per the threshold (exact trim; `~` accepted as an
/// approximation that removes at least to the threshold — Redis may remove
/// more). Surviving entries keep id + body; PEL records for removed ids go.
fn apply_trim(s: &mut StreamValue, spec: &TrimSpec) -> u64 {
    let mut removed = 0u64;
    loop {
        let too_long = match &spec.kind {
            TrimKind::MaxLen(n) => s.entries.len() as u64 > *n,
            TrimKind::MinId(min) => match s.entries.keys().next() {
                Some(first) => first < min,
                None => false,
            },
        };
        if !too_long {
            break;
        }
        let Some(first) = s.entries.keys().next().copied() else {
            break;
        };
        s.entries.remove(&first);
        drop_pending(s, first);
        removed += 1;
    }
    removed
}

// ---------------------------------------------------------------------------
// XADD
// ---------------------------------------------------------------------------

fn cmd_xadd(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() < 4 {
        return render::arity("xadd");
    }
    let key = args[0];
    let mut nomkstream = false;
    let mut order_opt: Option<StreamOrder> = None;
    let mut trim: Option<TrimSpec> = None;
    let mut i = 1;
    // Options until the id token.
    while i < args.len() {
        let opt = args[i].to_ascii_uppercase();
        match opt.as_str() {
            "NOMKSTREAM" => {
                nomkstream = true;
                i += 1;
            }
            "MAXLEN" | "MINID" => match parse_trim(args, i) {
                Some((spec, ni)) => {
                    trim = Some(spec);
                    i = ni;
                }
                None => return render::error(INVALID_ID),
            },
            "LIMIT" => i += 2,
            "ORDER" => {
                let Some(v) = args.get(i + 1) else {
                    return render::error("ERR syntax error");
                };
                order_opt = Some(match v.to_ascii_uppercase().as_str() {
                    "TOTAL" => StreamOrder::Total,
                    "PRODUCER" => StreamOrder::Producer,
                    _ => return render::error("ERR syntax error"),
                });
                i += 2;
            }
            _ => break,
        }
    }
    let Some(id_tok) = args.get(i) else {
        return render::arity("xadd");
    };
    let add = match parse_add_id(id_tok) {
        Some(a) => a,
        None => return render::error(INVALID_ID),
    };
    let fields = &args[i + 1..];
    if fields.is_empty() || fields.len() % 2 != 0 {
        return render::error("ERR wrong number of arguments for 'xadd'");
    }

    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let existing = match load_stream(session, id, key) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let (mut entry, mut stream, created) = match existing {
        Some((entry, stream)) => {
            if order_opt.is_some() {
                return render::error("ERR ORDER option is only valid when creating a stream");
            }
            (entry, stream, false)
        }
        None => {
            if nomkstream {
                return RedisReply::Bulk(None);
            }
            let order = order_opt.unwrap_or(StreamOrder::Total);
            match order {
                StreamOrder::Total => {
                    let lease = match session.stream_leases.grant(key) {
                        Ok(e) => e,
                        Err(e) => return lease_error("xadd", e),
                    };
                    (fresh_stream_entry(key, order, Some(lease)), StreamValue::new_total(lease), true)
                }
                StreamOrder::Producer => (
                    fresh_stream_entry(key, order, None),
                    StreamValue::new_producer(),
                    true,
                ),
            }
        }
    };

    // Total-order append fence: the presented epoch must be current.
    if !created && stream.order == StreamOrder::Total {
        let presented = stream.lease.unwrap_or(0);
        if let Err(e) = session.stream_leases.check(key, presented) {
            return lease_error("xadd", e);
        }
    }

    let new_id = match generate_id(add, stream.last_id) {
        Ok(id) => id,
        Err(msg) => return render::error(msg),
    };
    let body = StreamEntry {
        id: new_id,
        fields: fields_from(fields),
    };
    stream.entries.insert(new_id, body);
    stream.last_id = new_id;
    if let Some(spec) = &trim {
        apply_trim(&mut stream, spec);
    }
    if let Err(e) = store_stream(session, id, &mut entry, stream) {
        return e;
    }
    id_bulk(new_id)
}

// ---------------------------------------------------------------------------
// XDEL / XTRIM / XLEN
// ---------------------------------------------------------------------------

fn cmd_xdel(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() < 2 {
        return render::arity("xdel");
    }
    let key = args[0];
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (mut entry, mut stream) = match load_stream(session, id, key) {
        Ok(Some(v)) => v,
        Ok(None) => return RedisReply::Integer(0),
        Err(e) => return e,
    };
    let mut n = 0i64;
    for tok in &args[1..] {
        let Some(rid) = parse_simple_id(tok) else {
            return render::error(INVALID_ID);
        };
        if stream.entries.remove(&rid).is_some() {
            drop_pending(&mut stream, rid);
            n += 1;
        }
    }
    if n > 0 {
        if let Err(e) = store_stream(session, id, &mut entry, stream) {
            return e;
        }
    }
    RedisReply::Integer(n)
}

fn cmd_xtrim(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() < 3 {
        return render::arity("xtrim");
    }
    let key = args[0];
    let Some((spec, _)) = parse_trim(args, 1) else {
        return render::error(INVALID_ID);
    };
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (mut entry, mut stream) = match load_stream(session, id, key) {
        Ok(Some(v)) => v,
        Ok(None) => return RedisReply::Integer(0),
        Err(e) => return e,
    };
    let removed = apply_trim(&mut stream, &spec);
    if removed > 0 {
        if let Err(e) = store_stream(session, id, &mut entry, stream) {
            return e;
        }
    }
    RedisReply::Integer(removed as i64)
}

fn cmd_xlen(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 1 {
        return render::arity("xlen");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    match load_stream(session, id, args[0]) {
        Ok(Some((_, s))) => RedisReply::Integer(s.entries.len() as i64),
        Ok(None) => RedisReply::Integer(0),
        Err(e) => e,
    }
}

// ---------------------------------------------------------------------------
// XRANGE / XREVRANGE
// ---------------------------------------------------------------------------

fn cmd_xrange(session: &mut SessionState, args: &[&str], rev: bool) -> RedisReply {
    let name = if rev { "xrevrange" } else { "xrange" };
    if args.len() < 3 {
        return render::arity(name);
    }
    let count = parse_count_opt(&args[3..]);
    let (start_tok, end_tok) = if rev { (args[2], args[1]) } else { (args[1], args[2]) };
    let Some((start, s_excl)) = parse_range_bound(start_tok, true) else {
        return render::error(INVALID_ID);
    };
    let Some((end, e_excl)) = parse_range_bound(end_tok, false) else {
        return render::error(INVALID_ID);
    };
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let stream = match load_stream(session, id, args[0]) {
        Ok(Some((_, s))) => s,
        Ok(None) => return RedisReply::Array(vec![]),
        Err(e) => return e,
    };
    let mut items: Vec<&StreamEntry> = stream
        .entries
        .values()
        .filter(|e| range_contains(e.id, start, s_excl, end, e_excl))
        .collect();
    if rev {
        items.reverse();
    }
    if let Some(c) = count {
        items.truncate(c);
    }
    RedisReply::Array(items.into_iter().map(entry_reply).collect())
}

/// Parse a `XRANGE`/`XREVRANGE` bound (`-`, `+`, or id, optional `(`).
fn parse_range_bound(tok: &str, is_start: bool) -> Option<(StreamId, bool)> {
    match tok {
        "-" => Some((MIN_ID, false)),
        "+" => Some((MAX_ID, false)),
        _ => parse_range_id(tok, is_start),
    }
}

fn range_contains(id: StreamId, start: StreamId, s_excl: bool, end: StreamId, e_excl: bool) -> bool {
    let lo_ok = if s_excl { id > start } else { id >= start };
    let hi_ok = if e_excl { id < end } else { id <= end };
    lo_ok && hi_ok
}

/// Parse a `COUNT n` option from a trailing option slice.
fn parse_count_opt(opts: &[&str]) -> Option<usize> {
    let mut i = 0;
    while i < opts.len() {
        if opts[i].eq_ignore_ascii_case("COUNT") {
            return opts.get(i + 1).and_then(|s| s.parse().ok());
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// XREAD / XREADGROUP
// ---------------------------------------------------------------------------

/// Parse `XREAD`/`XREADGROUP` options + `STREAMS` into an [`XReadSpec`].
fn parse_read_args(kind: BlockKind, args: &[&str]) -> Result<XReadSpec, RedisReply> {
    let is_group = matches!(kind, BlockKind::XReadGroup);
    let mut count = None;
    let mut group = None;
    let mut noack = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].to_ascii_uppercase().as_str() {
            "COUNT" => {
                count = args.get(i + 1).and_then(|s| s.parse().ok());
                i += 2;
            }
            "BLOCK" => i += 2, // sync path never parks
            "GROUP" => {
                let (Some(g), Some(c)) = (args.get(i + 1), args.get(i + 2)) else {
                    return Err(render::error("ERR syntax error"));
                };
                group = Some((
                    Bytes::from(g.as_bytes().to_vec()),
                    Bytes::from(c.as_bytes().to_vec()),
                ));
                i += 3;
            }
            "NOACK" => {
                noack = true;
                i += 1;
            }
            "STREAMS" => {
                let rest = &args[i + 1..];
                if rest.is_empty() || rest.len() % 2 != 0 {
                    return Err(render::error("ERR Unbalanced XREAD list of streams"));
                }
                let half = rest.len() / 2;
                let keys = rest[..half]
                    .iter()
                    .map(|k| Bytes::from(k.as_bytes().to_vec()))
                    .collect();
                let ids = rest[half..].iter().map(|s| s.to_string()).collect();
                if is_group && group.is_none() {
                    return Err(render::error("ERR syntax error"));
                }
                return Ok(XReadSpec {
                    keys,
                    ids,
                    count,
                    group,
                    noack,
                });
            }
            _ => return Err(render::error("ERR syntax error")),
        }
    }
    Err(render::error("ERR syntax error"))
}

/// Resolve `$` id tokens to the stream's current `last_id` (stock once-at-park
/// semantics). No-op for other tokens.
pub fn resolve_dollar(session: &mut SessionState, req: &mut BlockRequest) {
    let Some(spec) = req.xread.as_mut() else {
        return;
    };
    let Ok(id) = session.keyspace_id() else {
        return;
    };
    for (k, tok) in spec.keys.iter().zip(spec.ids.iter_mut()) {
        if tok == "$" {
            let key = String::from_utf8_lossy(k).into_owned();
            if let Ok(Some((_, s))) = load_stream(session, id, &key) {
                *tok = fmt_id(s.last_id);
            } else {
                *tok = fmt_id(MIN_ID);
            }
        }
    }
}

/// Non-blocking `XREAD`. `Ok(None)` = no stream has new entries.
fn do_xread(session: &mut SessionState, id: ContainerId, spec: &XReadSpec) -> Result<Option<RedisReply>, RedisReply> {
    let mut streams_out = Vec::new();
    for (k, tok) in spec.keys.iter().zip(spec.ids.iter()) {
        let key = String::from_utf8_lossy(k).into_owned();
        // `$` in the sync path resolves to last id now; `>` is not valid for XREAD.
        let after = if tok == "$" {
            match load_stream(session, id, &key)? {
                Some((_, s)) => s.last_id,
                None => continue,
            }
        } else if tok == ">" {
            return Err(render::error(
                "ERR The $ ID is meaningless in the context of XREADGROUP",
            ));
        } else {
            match parse_simple_id(tok) {
                Some(v) => v,
                None => return Err(render::error(INVALID_ID)),
            }
        };
        let (_, s) = match load_stream(session, id, &key)? {
            Some(v) => v,
            None => continue,
        };
        let mut items: Vec<RedisReply> = s
            .entries
            .values()
            .filter(|e| e.id > after)
            .map(entry_reply)
            .collect();
        if let Some(c) = spec.count {
            items.truncate(c);
        }
        if !items.is_empty() {
            streams_out.push(RedisReply::Array(vec![
                RedisReply::Bulk(Some(k.to_vec())),
                RedisReply::Array(items),
            ]));
        }
    }
    if streams_out.is_empty() {
        Ok(None)
    } else {
        Ok(Some(RedisReply::Array(streams_out)))
    }
}

/// Non-blocking `XREADGROUP` (data-model §4: at-most-one-consumer delivery).
fn do_xreadgroup(
    session: &mut SessionState,
    id: ContainerId,
    spec: &XReadSpec,
) -> Result<Option<RedisReply>, RedisReply> {
    let (group_name, consumer) = spec.group.clone().expect("XREADGROUP requires GROUP");
    let mut streams_out = Vec::new();
    for (k, tok) in spec.keys.iter().zip(spec.ids.iter()) {
        let key = String::from_utf8_lossy(k).into_owned();
        let (mut entry, mut s) = match load_stream(session, id, &key)? {
            Some(v) => v,
            None => return Err(no_group(&String::from_utf8_lossy(&group_name), &key)),
        };
        let mut items: Vec<RedisReply> = Vec::new();
        {
            let Some(g) = s.groups.get_mut(&group_name) else {
                return Err(no_group(&String::from_utf8_lossy(&group_name), &key));
            };
            g.consumers.insert(consumer.clone());
            if tok == ">" {
                let after = g.last_delivered;
                let fresh: Vec<StreamEntry> = s
                    .entries
                    .values()
                    .filter(|e| e.id > after)
                    .cloned()
                    .collect();
                let mut last = after;
                for e in fresh.into_iter().take(spec.count.unwrap_or(usize::MAX)) {
                    last = e.id;
                    if !spec.noack {
                        g.pending.insert(
                            e.id,
                            PendingEntry {
                                id: e.id,
                                consumer: consumer.clone(),
                                delivery_count: 1,
                                delivered_at: now_unix_millis(),
                                body: e.clone(),
                            },
                        );
                    }
                    items.push(entry_reply(&e));
                }
                g.last_delivered = last;
            } else {
                let after = parse_simple_id(tok).ok_or_else(|| render::error(INVALID_ID))?;
                let mine: Vec<StreamEntry> = g
                    .pending
                    .values()
                    .filter(|p| p.consumer == consumer && p.id > after)
                    .map(|p| p.body.clone())
                    .collect();
                for e in mine.into_iter().take(spec.count.unwrap_or(usize::MAX)) {
                    items.push(entry_reply(&e));
                }
            }
        }
        if let Err(e) = store_stream(session, id, &mut entry, s) {
            return Err(e);
        }
        if !items.is_empty() {
            streams_out.push(RedisReply::Array(vec![
                RedisReply::Bulk(Some(k.to_vec())),
                RedisReply::Array(items),
            ]));
        }
    }
    if streams_out.is_empty() {
        Ok(None)
    } else {
        Ok(Some(RedisReply::Array(streams_out)))
    }
}

fn cmd_xread(session: &mut SessionState, args: &[&str], kind: BlockKind) -> RedisReply {
    let spec = match parse_read_args(kind, args) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let result = if matches!(kind, BlockKind::XReadGroup) {
        do_xreadgroup(session, id, &spec)
    } else {
        do_xread(session, id, &spec)
    };
    match result {
        Ok(Some(r)) => r,
        Ok(None) => RedisReply::NullArray,
        Err(e) => e,
    }
}

// ---------------------------------------------------------------------------
// XACK / XPENDING / XCLAIM
// ---------------------------------------------------------------------------

fn cmd_xack(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() < 3 {
        return render::arity("xack");
    }
    let key = args[0];
    let group_name = Bytes::from(args[1].as_bytes().to_vec());
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (mut entry, mut s) = match load_stream(session, id, key) {
        Ok(Some(v)) => v,
        Ok(None) => return RedisReply::Integer(0),
        Err(e) => return e,
    };
    let Some(g) = s.groups.get_mut(&group_name) else {
        return no_group(&String::from_utf8_lossy(&group_name), key);
    };
    let mut n = 0i64;
    for tok in &args[2..] {
        let Some(rid) = parse_simple_id(tok) else {
            return render::error(INVALID_ID);
        };
        if g.pending.remove(&rid).is_some() {
            n += 1;
        }
    }
    if n > 0 {
        if let Err(e) = store_stream(session, id, &mut entry, s) {
            return e;
        }
    }
    RedisReply::Integer(n)
}

fn cmd_xpending(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() < 2 {
        return render::arity("xpending");
    }
    let key = args[0];
    let group_name = Bytes::from(args[1].as_bytes().to_vec());
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let s = match load_stream(session, id, key) {
        Ok(Some((_, s))) => s,
        Ok(None) => return empty_pending_summary(),
        Err(e) => return e,
    };
    let Some(g) = s.groups.get(&group_name) else {
        return no_group(&String::from_utf8_lossy(&group_name), key);
    };
    // Summary form.
    if args.len() == 2 {
        return pending_summary(g);
    }
    // Extended form: start end count [consumer]
    if args.len() < 5 {
        return render::arity("xpending");
    }
    let Some((start, s_excl)) = parse_range_bound(args[2], true) else {
        return render::error(INVALID_ID);
    };
    let Some((end, e_excl)) = parse_range_bound(args[3], false) else {
        return render::error(INVALID_ID);
    };
    let count: i64 = match args[4].parse() {
        Ok(n) => n,
        Err(_) => return render::error("ERR value is not an integer or out of range"),
    };
    let consumer = args.get(5).map(|c| c.as_bytes().to_vec());
    let now = now_unix_millis();
    let mut out = Vec::new();
    for p in g.pending.values() {
        if !range_contains(p.id, start, s_excl, end, e_excl) {
            continue;
        }
        if let Some(c) = &consumer {
            if p.consumer.as_ref() != c.as_slice() {
                continue;
            }
        }
        out.push(RedisReply::Array(vec![
            id_bulk(p.id),
            RedisReply::Bulk(Some(p.consumer.to_vec())),
            RedisReply::Integer(now.saturating_sub(p.delivered_at)),
            RedisReply::Integer(p.delivery_count as i64),
        ]));
        if count >= 0 && out.len() as i64 >= count {
            break;
        }
    }
    RedisReply::Array(out)
}

fn empty_pending_summary() -> RedisReply {
    RedisReply::Array(vec![
        RedisReply::Integer(0),
        RedisReply::Bulk(None),
        RedisReply::Bulk(None),
        RedisReply::Bulk(None),
    ])
}

fn pending_summary(g: &ConsumerGroup) -> RedisReply {
    if g.pending.is_empty() {
        return empty_pending_summary();
    }
    let min = g.pending.keys().next().copied().unwrap_or(MIN_ID);
    let max = g.pending.keys().next_back().copied().unwrap_or(MIN_ID);
    let mut per: BTreeMap<&Bytes, i64> = BTreeMap::new();
    for p in g.pending.values() {
        *per.entry(&p.consumer).or_insert(0) += 1;
    }
    let consumers: Vec<RedisReply> = per
        .into_iter()
        .map(|(c, n)| {
            RedisReply::Array(vec![RedisReply::Bulk(Some(c.to_vec())), RedisReply::Integer(n)])
        })
        .collect();
    RedisReply::Array(vec![
        RedisReply::Integer(g.pending.len() as i64),
        id_bulk(min),
        id_bulk(max),
        RedisReply::Array(consumers),
    ])
}

fn cmd_xclaim(session: &mut SessionState, args: &[&str]) -> RedisReply {
    // XCLAIM key group consumer min-idle-time id [id...] [options]
    if args.len() < 5 {
        return render::arity("xclaim");
    }
    let key = args[0];
    let group_name = Bytes::from(args[1].as_bytes().to_vec());
    let consumer = Bytes::from(args[2].as_bytes().to_vec());
    let Some(min_idle) = args[3].parse::<i64>().ok() else {
        return render::error("ERR Invalid min-idle-time argument for XCLAIM");
    };
    let mut justid = false;
    let mut force = false;
    let mut idle_override: Option<i64> = None;
    let mut retrycount: Option<u64> = None;
    let mut i = 4;
    let mut ids: Vec<StreamId> = Vec::new();
    while i < args.len() {
        let opt = args[i].to_ascii_uppercase();
        match opt.as_str() {
            "IDLE" => {
                idle_override = args.get(i + 1).and_then(|s| s.parse().ok());
                i += 2;
            }
            "TIME" => {
                if let Some(t) = args.get(i + 1).and_then(|s| s.parse::<i64>().ok()) {
                    idle_override = Some(now_unix_millis().saturating_sub(t));
                }
                i += 2;
            }
            "RETRYCOUNT" => {
                retrycount = args.get(i + 1).and_then(|s| s.parse().ok());
                i += 2;
            }
            "FORCE" => {
                force = true;
                i += 1;
            }
            "JUSTID" => {
                justid = true;
                i += 1;
            }
            "LASTID" => i += 2,
            _ => {
                if let Some(rid) = parse_simple_id(args[i]) {
                    ids.push(rid);
                    i += 1;
                } else {
                    return render::error(INVALID_ID);
                }
            }
        }
    }
    if ids.is_empty() {
        return render::error("ERR wrong number of arguments for 'xclaim'");
    }

    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (mut entry, mut s) = match load_stream(session, id, key) {
        Ok(Some(v)) => v,
        Ok(None) => return no_group(&String::from_utf8_lossy(&group_name), key),
        Err(e) => return e,
    };
    let now = now_unix_millis();
    let mut out = Vec::new();
    for rid in ids {
        let existing = s
            .groups
            .get(&group_name)
            .and_then(|g| g.pending.get(&rid))
            .cloned();
        let proceed = match &existing {
            Some(p) => force || now.saturating_sub(p.delivered_at) >= min_idle,
            None => force,
        };
        if !proceed {
            continue;
        }
        let body = match existing {
            Some(p) => p.body,
            None => match s.entries.get(&rid) {
                Some(e) => e.clone(),
                None => continue,
            },
        };
        let Some(g) = s.groups.get_mut(&group_name) else {
            return no_group(&String::from_utf8_lossy(&group_name), key);
        };
        g.consumers.insert(consumer.clone());
        let prev_count = g.pending.get(&rid).map(|p| p.delivery_count).unwrap_or(0);
        let delivered_at = match idle_override {
            Some(ms) => now.saturating_sub(ms),
            None => now,
        };
        let count = retrycount.unwrap_or(prev_count + 1);
        g.pending.insert(
            rid,
            PendingEntry {
                id: rid,
                consumer: consumer.clone(),
                delivery_count: count,
                delivered_at,
                body: body.clone(),
            },
        );
        if justid {
            out.push(id_bulk(rid));
        } else {
            out.push(entry_reply(&body));
        }
    }
    if let Err(e) = store_stream(session, id, &mut entry, s) {
        return e;
    }
    RedisReply::Array(out)
}

// ---------------------------------------------------------------------------
// XGROUP
// ---------------------------------------------------------------------------

fn cmd_xgroup(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.is_empty() {
        return render::arity("xgroup");
    }
    let sub = args[0].to_ascii_uppercase();
    match sub.as_str() {
        "HELP" => RedisReply::Array(
            [
                "XGROUP <subcommand> [<arg> [value] [opt] ...]. Subcommands are:",
                "CREATE <key> <groupname> <id|$> [MKSTREAM]",
                "DESTROY <key> <groupname>",
                "SETID <key> <groupname> <id|$>",
                "DELCONSUMER <key> <groupname> <consumername>",
                "CREATECONSUMER <key> <groupname> <consumername>",
                "HELP",
            ]
            .iter()
            .map(|s| RedisReply::Bulk(Some(s.as_bytes().to_vec())))
            .collect(),
        ),
        "CREATE" => xgroup_create(session, &args[1..]),
        "DESTROY" => xgroup_destroy(session, &args[1..]),
        "SETID" => xgroup_setid(session, &args[1..]),
        "DELCONSUMER" => xgroup_delconsumer(session, &args[1..]),
        "CREATECONSUMER" => xgroup_createconsumer(session, &args[1..]),
        _ => render::unknown_subcommand(),
    }
}

fn xgroup_create(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() < 3 {
        return render::arity("xgroup");
    }
    let key = args[0];
    let group_name = Bytes::from(args[1].as_bytes().to_vec());
    let id_tok = args[2];
    let mkstream = args[3..].iter().any(|a| a.eq_ignore_ascii_case("MKSTREAM"));

    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let existing = match load_stream(session, id, key) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let (mut entry, mut stream) = match existing {
        Some(v) => v,
        None => {
            if !mkstream {
                return render::error(
                    "ERR The XGROUP subcommand requires the key to exist. Note that for CREATE you may want to use the MKSTREAM option to create an empty stream automatically.",
                );
            }
            let lease = match session.stream_leases.grant(key) {
                Ok(e) => e,
                Err(e) => return lease_error("xgroup", e),
            };
            (
                fresh_stream_entry(key, StreamOrder::Total, Some(lease)),
                StreamValue::new_total(lease),
            )
        }
    };
    if stream.groups.contains_key(&group_name) {
        return render::error("BUSYGROUP Consumer Group name already exists");
    }
    let last = if id_tok == "$" {
        stream.last_id
    } else {
        match parse_simple_id(id_tok) {
            Some(v) => v,
            None => return render::error(INVALID_ID),
        }
    };
    stream
        .groups
        .insert(group_name.clone(), ConsumerGroup::new(group_name, last));
    match store_stream(session, id, &mut entry, stream) {
        Ok(()) => RedisReply::Ok,
        Err(e) => e,
    }
}

fn xgroup_destroy(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 2 {
        return render::arity("xgroup");
    }
    let key = args[0];
    let group_name = Bytes::from(args[1].as_bytes().to_vec());
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (mut entry, mut stream) = match load_stream(session, id, key) {
        Ok(Some(v)) => v,
        Ok(None) => return RedisReply::Integer(0),
        Err(e) => return e,
    };
    let removed = stream.groups.remove(&group_name).is_some();
    if removed {
        if let Err(e) = store_stream(session, id, &mut entry, stream) {
            return e;
        }
    }
    RedisReply::Integer(removed as i64)
}

fn xgroup_setid(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() < 3 {
        return render::arity("xgroup");
    }
    let key = args[0];
    let group_name = Bytes::from(args[1].as_bytes().to_vec());
    let id_tok = args[2];
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (mut entry, mut stream) = match load_stream(session, id, key) {
        Ok(Some(v)) => v,
        Ok(None) => return no_group(&String::from_utf8_lossy(&group_name), key),
        Err(e) => return e,
    };
    let last_stream = stream.last_id;
    let Some(g) = stream.groups.get_mut(&group_name) else {
        return no_group(&String::from_utf8_lossy(&group_name), key);
    };
    let last = if id_tok == "$" {
        last_stream
    } else {
        match parse_simple_id(id_tok) {
            Some(v) => v,
            None => return render::error(INVALID_ID),
        }
    };
    g.last_delivered = last;
    match store_stream(session, id, &mut entry, stream) {
        Ok(()) => RedisReply::Ok,
        Err(e) => e,
    }
}

fn xgroup_delconsumer(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 3 {
        return render::arity("xgroup");
    }
    let key = args[0];
    let group_name = Bytes::from(args[1].as_bytes().to_vec());
    let consumer = Bytes::from(args[2].as_bytes().to_vec());
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (mut entry, mut stream) = match load_stream(session, id, key) {
        Ok(Some(v)) => v,
        Ok(None) => return no_group(&String::from_utf8_lossy(&group_name), key),
        Err(e) => return e,
    };
    let Some(g) = stream.groups.get_mut(&group_name) else {
        return no_group(&String::from_utf8_lossy(&group_name), key);
    };
    let doomed: Vec<StreamId> = g
        .pending
        .iter()
        .filter(|(_, p)| p.consumer == consumer)
        .map(|(k, _)| *k)
        .collect();
    let n = doomed.len() as i64;
    for k in doomed {
        g.pending.remove(&k);
    }
    g.consumers.remove(&consumer);
    if let Err(e) = store_stream(session, id, &mut entry, stream) {
        return e;
    }
    RedisReply::Integer(n)
}

fn xgroup_createconsumer(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 3 {
        return render::arity("xgroup");
    }
    let key = args[0];
    let group_name = Bytes::from(args[1].as_bytes().to_vec());
    let consumer = Bytes::from(args[2].as_bytes().to_vec());
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (mut entry, mut stream) = match load_stream(session, id, key) {
        Ok(Some(v)) => v,
        Ok(None) => return no_group(&String::from_utf8_lossy(&group_name), key),
        Err(e) => return e,
    };
    let Some(g) = stream.groups.get_mut(&group_name) else {
        return no_group(&String::from_utf8_lossy(&group_name), key);
    };
    let created = g.consumers.insert(consumer);
    if let Err(e) = store_stream(session, id, &mut entry, stream) {
        return e;
    }
    RedisReply::Integer(created as i64)
}

// ---------------------------------------------------------------------------
// XINFO
// ---------------------------------------------------------------------------

fn cmd_xinfo(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.is_empty() {
        return render::arity("xinfo");
    }
    match args[0].to_ascii_uppercase().as_str() {
        "HELP" => RedisReply::Array(
            [
                "XINFO <subcommand> [<arg> [value] [opt] ...]. Subcommands are:",
                "STREAM <key> [FULL [COUNT <count>]]",
                "GROUPS <key>",
                "CONSUMERS <key> <groupname>",
                "HELP",
            ]
            .iter()
            .map(|s| RedisReply::Bulk(Some(s.as_bytes().to_vec())))
            .collect(),
        ),
        "STREAM" => xinfo_stream(session, &args[1..]),
        "GROUPS" => xinfo_groups(session, &args[1..]),
        "CONSUMERS" => xinfo_consumers(session, &args[1..]),
        _ => render::unknown_subcommand(),
    }
}

fn xinfo_stream(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.is_empty() {
        return render::arity("xinfo");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let s = match load_stream(session, id, args[0]) {
        Ok(Some((_, s))) => s,
        Ok(None) => return render::error("ERR no such key"),
        Err(e) => return e,
    };
    let first = s.entries.values().next();
    let last = s.entries.values().next_back();
    let mut out = vec![
        RedisReply::Bulk(Some(b"length".to_vec())),
        RedisReply::Integer(s.entries.len() as i64),
        RedisReply::Bulk(Some(b"radix-tree-keys".to_vec())),
        RedisReply::Integer(1),
        RedisReply::Bulk(Some(b"radix-tree-nodes".to_vec())),
        RedisReply::Integer(2),
        RedisReply::Bulk(Some(b"last-generated-id".to_vec())),
        id_bulk(s.last_id),
        RedisReply::Bulk(Some(b"max-deleted-entry-id".to_vec())),
        RedisReply::Bulk(Some(b"0-0".to_vec())),
        RedisReply::Bulk(Some(b"entries-added".to_vec())),
        RedisReply::Integer(s.entries.len() as i64),
        RedisReply::Bulk(Some(b"recorded-first-entry-id".to_vec())),
        id_bulk(first.map(|e| e.id).unwrap_or(MIN_ID)),
        RedisReply::Bulk(Some(b"groups".to_vec())),
        RedisReply::Integer(s.groups.len() as i64),
        RedisReply::Bulk(Some(b"first-entry".to_vec())),
        first.map(entry_reply).unwrap_or(RedisReply::Bulk(None)),
        RedisReply::Bulk(Some(b"last-entry".to_vec())),
        last.map(entry_reply).unwrap_or(RedisReply::Bulk(None)),
    ];
    RedisReply::Array(std::mem::take(&mut out))
}

fn xinfo_groups(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.is_empty() {
        return render::arity("xinfo");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let s = match load_stream(session, id, args[0]) {
        Ok(Some((_, s))) => s,
        Ok(None) => return render::error("ERR no such key"),
        Err(e) => return e,
    };
    let groups: Vec<RedisReply> = s
        .groups
        .values()
        .map(|g| {
            RedisReply::Array(vec![
                RedisReply::Bulk(Some(b"name".to_vec())),
                RedisReply::Bulk(Some(g.name.to_vec())),
                RedisReply::Bulk(Some(b"consumers".to_vec())),
                RedisReply::Integer(g.consumers.len() as i64),
                RedisReply::Bulk(Some(b"pending".to_vec())),
                RedisReply::Integer(g.pending.len() as i64),
                RedisReply::Bulk(Some(b"last-delivered-id".to_vec())),
                id_bulk(g.last_delivered),
                RedisReply::Bulk(Some(b"entries-read".to_vec())),
                RedisReply::Bulk(None),
                RedisReply::Bulk(Some(b"lag".to_vec())),
                RedisReply::Integer(0),
            ])
        })
        .collect();
    RedisReply::Array(groups)
}

fn xinfo_consumers(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() < 2 {
        return render::arity("xinfo");
    }
    let group_name = Bytes::from(args[1].as_bytes().to_vec());
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let s = match load_stream(session, id, args[0]) {
        Ok(Some((_, s))) => s,
        Ok(None) => return no_group(&String::from_utf8_lossy(&group_name), args[0]),
        Err(e) => return e,
    };
    let Some(g) = s.groups.get(&group_name) else {
        return no_group(&String::from_utf8_lossy(&group_name), args[0]);
    };
    let now = now_unix_millis();
    let consumers: Vec<RedisReply> = g
        .consumers
        .iter()
        .map(|c| {
            let mine: Vec<&PendingEntry> =
                g.pending.values().filter(|p| &p.consumer == c).collect();
            let idle = mine
                .iter()
                .map(|p| now.saturating_sub(p.delivered_at))
                .max()
                .unwrap_or(0);
            RedisReply::Array(vec![
                RedisReply::Bulk(Some(b"name".to_vec())),
                RedisReply::Bulk(Some(c.to_vec())),
                RedisReply::Bulk(Some(b"pending".to_vec())),
                RedisReply::Integer(mine.len() as i64),
                RedisReply::Bulk(Some(b"idle".to_vec())),
                RedisReply::Integer(idle),
                RedisReply::Bulk(Some(b"inactive".to_vec())),
                RedisReply::Integer(idle),
            ])
        })
        .collect();
    RedisReply::Array(consumers)
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

pub fn dispatch(session: &mut SessionState, upper: &str, args: &[&str]) -> Option<RedisReply> {
    // Defensive: never let `XAUTOCLAIM` reach the stream struct (T041). The
    // compat matrix already classifies it MustNot, so this is belt-and-braces.
    if upper == "XAUTOCLAIM" {
        return Some(render::unknown_command("XAUTOCLAIM"));
    }
    match upper {
        "XADD" => Some(cmd_xadd(session, args)),
        "XLEN" => Some(cmd_xlen(session, args)),
        "XRANGE" => Some(cmd_xrange(session, args, false)),
        "XREVRANGE" => Some(cmd_xrange(session, args, true)),
        "XDEL" => Some(cmd_xdel(session, args)),
        "XTRIM" => Some(cmd_xtrim(session, args)),
        "XREAD" => Some(cmd_xread(session, args, BlockKind::XRead)),
        "XREADGROUP" => Some(cmd_xread(session, args, BlockKind::XReadGroup)),
        "XGROUP" => Some(cmd_xgroup(session, args)),
        "XACK" => Some(cmd_xack(session, args)),
        "XPENDING" => Some(cmd_xpending(session, args)),
        "XCLAIM" => Some(cmd_xclaim(session, args)),
        "XINFO" => Some(cmd_xinfo(session, args)),
        _ => None,
    }
}

/// Immediate attempt for a parked `XREAD`/`XREADGROUP … BLOCK` (`017` T036).
/// `Some` when data is available or the command has an immediate error;
/// `None` keeps the connection parked.
pub fn try_block(session: &mut SessionState, req: &BlockRequest) -> Option<RedisReply> {
    let spec = req.xread.as_ref()?;
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return Some(e),
    };
    let result = if matches!(req.kind, BlockKind::XReadGroup) {
        do_xreadgroup(session, id, spec)
    } else {
        do_xread(session, id, spec)
    };
    match result {
        Ok(Some(r)) => Some(r),
        Ok(None) => None,
        Err(e) => Some(e),
    }
}

//! Redis HyperLogLog family (`017` US5 T062). Sketches are backed by the
//! registered L0 `hyperloglog` primitive in `spacestorage_types::HyperLogLog`
//! and carried as an `EntryPayload::HyperLogLog` inside the namespace key-space
//! container. The handler never keeps a protocol-private encoding: it only
//! encodes/decodes the entry payload built from the shared type.

use bytes::Bytes;
use spacestorage_types::HyperLogLog;

use crate::lower::{EntryPayload, HllValue, RedisEntry};
use crate::render;
use super::{RedisReply, SessionState};

fn fresh_hll_entry(key: &str, hll: &HyperLogLog) -> RedisEntry {
    RedisEntry::new(
        Bytes::from(key.as_bytes().to_vec()),
        EntryPayload::HyperLogLog(HllValue {
            precision: hll.precision(),
            registers: hll.registers().to_vec(),
        }),
    )
}

/// Load one key as a sketch. A missing key is `None`; any non-HLL payload
/// (including a plain string) is `WRONGTYPE` (`017` data-model §8).
fn load_hll(
    session: &SessionState,
    id: spacestorage_types::ContainerId,
    key: &str,
) -> Result<Option<(RedisEntry, HyperLogLog)>, RedisReply> {
    match session.load_entry(id, key)? {
        None => Ok(None),
        Some(mut e) => match std::mem::replace(&mut e.payload, EntryPayload::Bytes(Vec::new())) {
            EntryPayload::HyperLogLog(h) => {
                Ok(Some((e, HyperLogLog::from_registers(h.precision, h.registers))))
            }
            other => {
                e.payload = other;
                Err(render::wrongtype())
            }
        },
    }
}

fn store_hll(
    session: &SessionState,
    id: spacestorage_types::ContainerId,
    entry: &mut RedisEntry,
    hll: &HyperLogLog,
) -> Result<(), RedisReply> {
    entry.replace(EntryPayload::HyperLogLog(HllValue {
        precision: hll.precision(),
        registers: hll.registers().to_vec(),
    }));
    session.store_entry(id, entry)
}

pub fn dispatch(session: &mut SessionState, upper: &str, args: &[&str]) -> Option<RedisReply> {
    match upper {
        "PFADD" => Some(cmd_pfadd(session, args)),
        "PFCOUNT" => Some(cmd_pfcount(session, args)),
        "PFMERGE" => Some(cmd_pfmerge(session, args)),
        _ => None,
    }
}

fn cmd_pfadd(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.is_empty() {
        return render::arity("pfadd");
    }
    let key = args[0];
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (mut entry, mut hll, existed) = match load_hll(session, id, key) {
        Ok(Some((e, h))) => (e, h, true),
        Ok(None) => (
            fresh_hll_entry(key, &HyperLogLog::default()),
            HyperLogLog::default(),
            false,
        ),
        Err(e) => return e,
    };
    let mut changed = false;
    for elem in &args[1..] {
        if hll.add(elem.as_bytes()) {
            changed = true;
        }
    }
    // Redis treats a newly created sketch as a change even with no elements;
    // an existing sketch with no register growth returns 0 and is not rewritten.
    if !existed || changed {
        if let Err(e) = store_hll(session, id, &mut entry, &hll) {
            return e;
        }
    }
    RedisReply::Integer(i64::from(!existed || changed))
}

fn cmd_pfcount(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.is_empty() {
        return render::arity("pfcount");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    if args.len() == 1 {
        return match load_hll(session, id, args[0]) {
            Ok(Some((_, hll))) => RedisReply::Integer(hll.count() as i64),
            Ok(None) => RedisReply::Integer(0),
            Err(e) => e,
        };
    }
    // Multi-key: union (register-wise max) of the present sketches, then count.
    let mut merged: Option<HyperLogLog> = None;
    for k in args {
        let loaded = match load_hll(session, id, k) {
            Ok(v) => v,
            Err(e) => return e,
        };
        if let Some((_, hll)) = loaded {
            match &mut merged {
                None => merged = Some(hll),
                Some(acc) => {
                    if hll.precision() > acc.precision() {
                        let mut higher = hll.clone();
                        higher.merge(acc);
                        *acc = higher;
                    } else {
                        acc.merge(&hll);
                    }
                }
            }
        }
    }
    match merged {
        Some(h) => RedisReply::Integer(h.count() as i64),
        None => RedisReply::Integer(0),
    }
}

fn cmd_pfmerge(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.is_empty() {
        return render::arity("pfmerge");
    }
    let dest = args[0];
    let sources = &args[1..];
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    // A non-HLL destination is a type error (the entry is replaced otherwise).
    if let Err(e) = load_hll(session, id, dest) {
        return e;
    }
    let mut max_precision = 0u8;
    let mut loaded: Vec<HyperLogLog> = Vec::with_capacity(sources.len());
    for k in sources {
        match load_hll(session, id, k) {
            Ok(Some((_, hll))) => {
                max_precision = max_precision.max(hll.precision());
                loaded.push(hll);
            }
            Ok(None) => {}
            Err(e) => return e,
        }
    }
    let precision = if max_precision == 0 {
        spacestorage_types::l0::HLL_DEFAULT_PRECISION
    } else {
        max_precision
    };
    let mut merged = HyperLogLog::new(precision);
    for hll in &loaded {
        merged.merge(hll);
    }
    // Empty source list clears the destination.
    let mut entry = fresh_hll_entry(dest, &merged);
    if let Err(e) = store_hll(session, id, &mut entry, &merged) {
        return e;
    }
    RedisReply::Ok
}

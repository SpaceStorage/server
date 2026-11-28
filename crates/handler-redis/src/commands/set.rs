//! Redis `set` family (`017` US1 T024).

use std::collections::BTreeSet;

use bytes::Bytes;

use crate::lower::{EntryPayload, RedisEntry, SetValue};
use crate::render;
use super::{RedisReply, SessionState};

fn load_set(
    session: &SessionState,
    id: spacestorage_types::ContainerId,
    key: &str,
) -> Result<Option<(RedisEntry, SetValue)>, RedisReply> {
    match session.load_entry(id, key)? {
        None => Ok(None),
        Some(mut e) => {
            if !matches!(e.payload, EntryPayload::Set(_)) {
                return Err(render::wrongtype());
            }
            let set = match std::mem::replace(&mut e.payload, EntryPayload::Bytes(Vec::new())) {
                EntryPayload::Set(s) => s,
                _ => unreachable!(),
            };
            Ok(Some((e, set)))
        }
    }
}

fn fresh_set_entry(key: &str) -> RedisEntry {
    RedisEntry::new(
        Bytes::from(key.as_bytes().to_vec()),
        EntryPayload::Set(SetValue::default()),
    )
}

fn store_set(
    session: &SessionState,
    id: spacestorage_types::ContainerId,
    entry: &mut RedisEntry,
    set: SetValue,
    key: &str,
) -> Result<(), RedisReply> {
    if set.members.is_empty() {
        entry.replace(EntryPayload::Set(set));
        session.delete_entry(id, key)?;
        Ok(())
    } else {
        entry.replace(EntryPayload::Set(set));
        session.store_entry(id, entry)
    }
}

pub fn dispatch(session: &mut SessionState, upper: &str, args: &[&str]) -> Option<RedisReply> {
    match upper {
        "SADD" => Some(cmd_sadd(session, args)),
        "SREM" => Some(cmd_srem(session, args)),
        "SMEMBERS" => Some(cmd_smembers(session, args)),
        "SISMEMBER" => Some(cmd_sismember(session, args)),
        "SCARD" => Some(cmd_scard(session, args)),
        "SPOP" => Some(cmd_spop(session, args)),
        "SRANDMEMBER" => Some(cmd_srandmember(session, args)),
        "SSCAN" => Some(cmd_sscan(session, args)),
        "SINTER" => Some(cmd_setop(session, args, SetOp::Inter)),
        "SUNION" => Some(cmd_setop(session, args, SetOp::Union)),
        "SDIFF" => Some(cmd_setop(session, args, SetOp::Diff)),
        "SINTERSTORE" => Some(cmd_setop_store(session, args, SetOp::Inter)),
        "SUNIONSTORE" => Some(cmd_setop_store(session, args, SetOp::Union)),
        "SDIFFSTORE" => Some(cmd_setop_store(session, args, SetOp::Diff)),
        _ => None,
    }
}

fn cmd_sadd(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() < 2 {
        return render::arity("sadd");
    }
    let key = args[0];
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (mut entry, mut set) = match load_set(session, id, key) {
        Ok(Some(v)) => v,
        Ok(None) => (fresh_set_entry(key), SetValue::default()),
        Err(e) => return e,
    };
    let mut added = 0i64;
    for m in &args[1..] {
        if set.members.insert(Bytes::from(m.as_bytes().to_vec())) {
            added += 1;
        }
    }
    if let Err(e) = store_set(session, id, &mut entry, set, key) {
        return e;
    }
    RedisReply::Integer(added)
}

fn cmd_srem(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() < 2 {
        return render::arity("srem");
    }
    let key = args[0];
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (mut entry, mut set) = match load_set(session, id, key) {
        Ok(Some(v)) => v,
        Ok(None) => return RedisReply::Integer(0),
        Err(e) => return e,
    };
    let mut n = 0i64;
    for m in &args[1..] {
        if set.members.remove(m.as_bytes()) {
            n += 1;
        }
    }
    if n > 0 {
        if let Err(e) = store_set(session, id, &mut entry, set, key) {
            return e;
        }
    }
    RedisReply::Integer(n)
}

fn cmd_smembers(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 1 {
        return render::arity("smembers");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    match load_set(session, id, args[0]) {
        Ok(Some((_, s))) => RedisReply::Array(
            s.members
                .into_iter()
                .map(|m| RedisReply::Bulk(Some(m.to_vec())))
                .collect(),
        ),
        Ok(None) => RedisReply::Array(vec![]),
        Err(e) => e,
    }
}

fn cmd_sismember(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 2 {
        return render::arity("sismember");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    match load_set(session, id, args[0]) {
        Ok(Some((_, s))) => RedisReply::Integer(if s.members.contains(args[1].as_bytes()) {
            1
        } else {
            0
        }),
        Ok(None) => RedisReply::Integer(0),
        Err(e) => e,
    }
}

fn cmd_scard(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 1 {
        return render::arity("scard");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    match load_set(session, id, args[0]) {
        Ok(Some((_, s))) => RedisReply::Integer(s.members.len() as i64),
        Ok(None) => RedisReply::Integer(0),
        Err(e) => e,
    }
}

fn cmd_spop(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.is_empty() {
        return render::arity("spop");
    }
    let key = args[0];
    let count = args.get(1).and_then(|s| s.parse::<i64>().ok());
    if args.len() > 1 && count.is_none() {
        return render::error("ERR value is not an integer or out of range");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (mut entry, mut set) = match load_set(session, id, key) {
        Ok(Some(v)) => v,
        Ok(None) => {
            return match count {
                Some(_) => RedisReply::Array(vec![]),
                None => RedisReply::Bulk(None),
            }
        }
        Err(e) => return e,
    };
    if count.is_none() {
        // Deterministic single pop.
        let member = set.members.iter().next().cloned();
        if let Some(m) = &member {
            set.members.remove(m);
            if let Err(e) = store_set(session, id, &mut entry, set, key) {
                return e;
            }
        }
        return RedisReply::Bulk(member.map(|m| m.to_vec()));
    }
    let n = count.unwrap().max(0) as usize;
    let chosen: Vec<Bytes> = set.members.iter().take(n).cloned().collect();
    for m in &chosen {
        set.members.remove(m);
    }
    if let Err(e) = store_set(session, id, &mut entry, set, key) {
        return e;
    }
    RedisReply::Array(
        chosen
            .into_iter()
            .map(|m| RedisReply::Bulk(Some(m.to_vec())))
            .collect(),
    )
}

fn cmd_srandmember(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.is_empty() {
        return render::arity("srandmember");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let members: Vec<Bytes> = match load_set(session, id, args[0]) {
        Ok(Some((_, s))) => s.members.into_iter().collect(),
        Ok(None) => Vec::new(),
        Err(e) => return e,
    };
    if args.len() == 1 {
        return RedisReply::Bulk(members.into_iter().next().map(|m| m.to_vec()));
    }
    let Some(count) = args[1].parse::<i64>().ok() else {
        return render::error("ERR value is not an integer or out of range");
    };
    let n = count.unsigned_abs() as usize;
    RedisReply::Array(
        members
            .into_iter()
            .take(n)
            .map(|m| RedisReply::Bulk(Some(m.to_vec())))
            .collect(),
    )
}

fn cmd_sscan(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() < 2 {
        return render::arity("sscan");
    }
    let mut i = 2;
    let mut count = 10usize;
    let mut pattern: Option<String> = None;
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
            _ => i += 1,
        }
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let members: Vec<Bytes> = match load_set(session, id, args[0]) {
        Ok(Some((_, s))) => s.members.into_iter().collect(),
        Ok(None) => Vec::new(),
        Err(e) => return e,
    };
    let items: Vec<RedisReply> = members
        .into_iter()
        .filter(|m| {
            pattern
                .as_deref()
                .map(|p| crate::lower::glob_match(p, &String::from_utf8_lossy(m)))
                .unwrap_or(true)
        })
        .take(count.max(1))
        .map(|m| RedisReply::Bulk(Some(m.to_vec())))
        .collect();
    RedisReply::Array(vec![
        RedisReply::Bulk(Some(b"0".to_vec())),
        RedisReply::Array(items),
    ])
}

enum SetOp {
    Inter,
    Union,
    Diff,
}

fn collect(session: &SessionState, id: spacestorage_types::ContainerId, keys: &[&str]) -> Result<Vec<BTreeSet<Bytes>>, RedisReply> {
    let mut out = Vec::new();
    for k in keys {
        match load_set(session, id, k) {
            Ok(Some((_, s))) => out.push(s.members),
            // A missing key behaves as the empty set for all three ops.
            Ok(None) => out.push(BTreeSet::new()),
            Err(e) => return Err(e),
        }
    }
    Ok(out)
}

fn compute(sets: &[BTreeSet<Bytes>], op: &SetOp) -> BTreeSet<Bytes> {
    if sets.is_empty() {
        return BTreeSet::new();
    }
    let mut acc = sets[0].clone();
    for s in &sets[1..] {
        match op {
            SetOp::Inter => acc = acc.intersection(s).cloned().collect(),
            SetOp::Union => acc = acc.union(s).cloned().collect(),
            SetOp::Diff => acc = acc.difference(s).cloned().collect(),
        }
    }
    acc
}

fn cmd_setop(session: &mut SessionState, args: &[&str], op: SetOp) -> RedisReply {
    if args.is_empty() {
        return render::arity("setop");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let sets = match collect(session, id, args) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let result = compute(&sets, &op);
    RedisReply::Array(
        result
            .into_iter()
            .map(|m| RedisReply::Bulk(Some(m.to_vec())))
            .collect(),
    )
}

fn cmd_setop_store(session: &mut SessionState, args: &[&str], op: SetOp) -> RedisReply {
    if args.len() < 2 {
        return render::arity("setopstore");
    }
    let dest = args[0];
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let sets = match collect(session, id, &args[1..]) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let result = compute(&sets, &op);
    let n = result.len() as i64;
    let mut entry = fresh_set_entry(dest);
    if let Err(e) = store_set(session, id, &mut entry, SetValue { members: result }, dest) {
        return e;
    }
    RedisReply::Integer(n)
}

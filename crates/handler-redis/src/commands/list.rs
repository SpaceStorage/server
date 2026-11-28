//! Redis `list` family (`017` US1 T023) incl. the blocking forms (T027). Every
//! list is one typed entry inside the namespace key-space container; the handler
//! keeps no private store.

use std::collections::VecDeque;

use bytes::Bytes;

use crate::block::{BlockKind, BlockRequest};
use crate::lower::{EntryPayload, ListValue, RedisEntry};
use crate::render;
use super::{RedisReply, SessionState};

type ContainerId = spacestorage_types::ContainerId;

fn load_list(
    session: &SessionState,
    id: ContainerId,
    key: &str,
) -> Result<Option<(RedisEntry, ListValue)>, RedisReply> {
    match session.load_entry(id, key)? {
        None => Ok(None),
        Some(mut e) => {
            if !matches!(e.payload, EntryPayload::List(_)) {
                return Err(render::wrongtype());
            }
            let list = match std::mem::replace(&mut e.payload, EntryPayload::Bytes(Vec::new())) {
                EntryPayload::List(l) => l,
                _ => unreachable!(),
            };
            Ok(Some((e, list)))
        }
    }
}

fn fresh_list_entry(key: &str) -> RedisEntry {
    RedisEntry::new(
        Bytes::from(key.as_bytes().to_vec()),
        EntryPayload::List(ListValue::default()),
    )
}

/// An empty list is removed from the key space (Redis drops empty list keys).
fn store_list(
    session: &SessionState,
    id: ContainerId,
    entry: &mut RedisEntry,
    list: ListValue,
    key: &str,
) -> Result<(), RedisReply> {
    let empty = list.elements.is_empty();
    entry.replace(EntryPayload::List(list));
    if empty {
        session.delete_entry(id, key)?;
        Ok(())
    } else {
        session.store_entry(id, entry)
    }
}

pub fn dispatch(session: &mut SessionState, upper: &str, args: &[&str]) -> Option<RedisReply> {
    match upper {
        "LPUSH" => Some(cmd_push(session, args, true, false)),
        "RPUSH" => Some(cmd_push(session, args, false, false)),
        "LPUSHX" => Some(cmd_push(session, args, true, true)),
        "RPUSHX" => Some(cmd_push(session, args, false, true)),
        "LPOP" => Some(cmd_pop(session, args, true)),
        "RPOP" => Some(cmd_pop(session, args, false)),
        "LLEN" => Some(cmd_llen(session, args)),
        "LRANGE" => Some(cmd_lrange(session, args)),
        "LINDEX" => Some(cmd_lindex(session, args)),
        "LSET" => Some(cmd_lset(session, args)),
        "LREM" => Some(cmd_lrem(session, args)),
        "LTRIM" => Some(cmd_ltrim(session, args)),
        "LINSERT" => Some(cmd_linsert(session, args)),
        "RPOPLPUSH" => Some(cmd_rpoplpush(session, args)),
        "LMOVE" => Some(cmd_lmove(session, args)),
        _ => None,
    }
}

fn cmd_push(session: &mut SessionState, args: &[&str], left: bool, only_if_exists: bool) -> RedisReply {
    let name = match (left, only_if_exists) {
        (true, false) => "lpush",
        (false, false) => "rpush",
        (true, true) => "lpushx",
        (false, true) => "rpushx",
    };
    if args.len() < 2 {
        return render::arity(name);
    }
    let key = args[0];
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (mut entry, mut list) = match load_list(session, id, key) {
        Ok(Some(v)) => v,
        Ok(None) => {
            if only_if_exists {
                return RedisReply::Integer(0);
            }
            (fresh_list_entry(key), ListValue::default())
        }
        Err(e) => return e,
    };
    for m in &args[1..] {
        let b = Bytes::from(m.as_bytes().to_vec());
        if left {
            list.elements.push_front(b);
        } else {
            list.elements.push_back(b);
        }
    }
    let len = list.elements.len() as i64;
    if let Err(e) = store_list(session, id, &mut entry, list, key) {
        return e;
    }
    RedisReply::Integer(len)
}

fn cmd_pop(session: &mut SessionState, args: &[&str], left: bool) -> RedisReply {
    let name = if left { "lpop" } else { "rpop" };
    if args.is_empty() {
        return render::arity(name);
    }
    let key = args[0];
    let count = match args.get(1) {
        None => None,
        Some(s) => match s.parse::<i64>() {
            Ok(n) => Some(n),
            Err(_) => return render::error("ERR value is not an integer or out of range"),
        },
    };
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (mut entry, mut list) = match load_list(session, id, key) {
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
        let v = if left {
            list.elements.pop_front()
        } else {
            list.elements.pop_back()
        };
        if let Err(e) = store_list(session, id, &mut entry, list, key) {
            return e;
        }
        return RedisReply::Bulk(v.map(|b| b.to_vec()));
    }
    let n = count.unwrap().max(0) as usize;
    let mut out = Vec::new();
    for _ in 0..n {
        let v = if left {
            list.elements.pop_front()
        } else {
            list.elements.pop_back()
        };
        match v {
            Some(b) => out.push(RedisReply::Bulk(Some(b.to_vec()))),
            None => break,
        }
    }
    if let Err(e) = store_list(session, id, &mut entry, list, key) {
        return e;
    }
    RedisReply::Array(out)
}

fn cmd_llen(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 1 {
        return render::arity("llen");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    match load_list(session, id, args[0]) {
        Ok(Some((_, l))) => RedisReply::Integer(l.elements.len() as i64),
        Ok(None) => RedisReply::Integer(0),
        Err(e) => e,
    }
}

/// Inclusive index range normalisation (Redis `LRANGE`/`LTRIM` rules).
fn norm_range(start: i64, stop: i64, len: i64) -> Option<(usize, usize)> {
    if len == 0 {
        return None;
    }
    let mut s = if start < 0 { start + len } else { start };
    let mut e = if stop < 0 { stop + len } else { stop };
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

/// Shared rank-range normalisation for sibling families (`LRANGE`/`ZRANGE` rank).
pub(crate) fn norm_for_rank(len: i64, start: i64, stop: i64) -> Option<(usize, usize)> {
    norm_range(start, stop, len)
}

fn cmd_lrange(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 3 {
        return render::arity("lrange");
    }
    let (Some(start), Some(stop)) = (args[1].parse::<i64>().ok(), args[2].parse::<i64>().ok()) else {
        return render::error("ERR value is not an integer or out of range");
    };
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    match load_list(session, id, args[0]) {
        Ok(Some((_, l))) => {
            let len = l.elements.len() as i64;
            let out = match norm_range(start, stop, len) {
                Some((s, e)) => (s..=e)
                    .map(|i| RedisReply::Bulk(Some(l.elements[i].to_vec())))
                    .collect(),
                None => Vec::new(),
            };
            RedisReply::Array(out)
        }
        Ok(None) => RedisReply::Array(vec![]),
        Err(e) => e,
    }
}

/// Index into a list allowing negative offsets.
fn at_index(len: usize, index: i64) -> Option<usize> {
    if index < 0 {
        let i = index + len as i64;
        if i < 0 {
            None
        } else {
            Some(i as usize)
        }
    } else if (index as usize) < len {
        Some(index as usize)
    } else {
        None
    }
}

fn cmd_lindex(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 2 {
        return render::arity("lindex");
    }
    let Some(index) = args[1].parse::<i64>().ok() else {
        return render::error("ERR value is not an integer or out of range");
    };
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    match load_list(session, id, args[0]) {
        Ok(Some((_, l))) => match at_index(l.elements.len(), index) {
            Some(i) => RedisReply::Bulk(Some(l.elements[i].to_vec())),
            None => RedisReply::Bulk(None),
        },
        Ok(None) => RedisReply::Bulk(None),
        Err(e) => e,
    }
}

fn cmd_lset(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 3 {
        return render::arity("lset");
    }
    let Some(index) = args[1].parse::<i64>().ok() else {
        return render::error("ERR value is not an integer or out of range");
    };
    let key = args[0];
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (mut entry, mut list) = match load_list(session, id, key) {
        Ok(Some(v)) => v,
        Ok(None) => return render::error("ERR no such key"),
        Err(e) => return e,
    };
    let Some(i) = at_index(list.elements.len(), index) else {
        return render::error("ERR index out of range");
    };
    list.elements[i] = Bytes::from(args[2].as_bytes().to_vec());
    match store_list(session, id, &mut entry, list, key) {
        Ok(()) => RedisReply::Ok,
        Err(e) => e,
    }
}

fn cmd_lrem(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 3 {
        return render::arity("lrem");
    }
    let Some(count) = args[1].parse::<i64>().ok() else {
        return render::error("ERR value is not an integer or out of range");
    };
    let key = args[0];
    let value = args[2].as_bytes();
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (mut entry, mut list) = match load_list(session, id, key) {
        Ok(Some(v)) => v,
        Ok(None) => return RedisReply::Integer(0),
        Err(e) => return e,
    };
    let mut removed = 0i64;
    if count >= 0 {
        let limit = if count == 0 { i64::MAX } else { count };
        let mut i = 0;
        while i < list.elements.len() && removed < limit {
            if list.elements[i].as_ref() == value {
                list.elements.remove(i);
                removed += 1;
            } else {
                i += 1;
            }
        }
    } else {
        let limit = -count;
        let mut i = list.elements.len();
        while i > 0 && removed < limit {
            i -= 1;
            if list.elements[i].as_ref() == value {
                list.elements.remove(i);
                removed += 1;
            }
        }
    }
    if removed > 0 {
        if let Err(e) = store_list(session, id, &mut entry, list, key) {
            return e;
        }
    }
    RedisReply::Integer(removed)
}

fn cmd_ltrim(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 3 {
        return render::arity("ltrim");
    }
    let (Some(start), Some(stop)) = (args[1].parse::<i64>().ok(), args[2].parse::<i64>().ok()) else {
        return render::error("ERR value is not an integer or out of range");
    };
    let key = args[0];
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (mut entry, mut list) = match load_list(session, id, key) {
        Ok(Some(v)) => v,
        Ok(None) => return RedisReply::Ok,
        Err(e) => return e,
    };
    let kept = match norm_range(start, stop, list.elements.len() as i64) {
        Some((s, e)) => list.elements.range(s..=e).cloned().collect::<VecDeque<_>>(),
        None => VecDeque::new(),
    };
    list.elements = kept;
    match store_list(session, id, &mut entry, list, key) {
        Ok(()) => RedisReply::Ok,
        Err(e) => e,
    }
}

fn cmd_linsert(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 4 {
        return render::arity("linsert");
    }
    let before = match args[1].to_ascii_uppercase().as_str() {
        "BEFORE" => true,
        "AFTER" => false,
        _ => return render::error("ERR syntax error"),
    };
    let key = args[0];
    let pivot = args[2].as_bytes();
    let value = Bytes::from(args[3].as_bytes().to_vec());
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (mut entry, mut list) = match load_list(session, id, key) {
        Ok(Some(v)) => v,
        Ok(None) => return RedisReply::Integer(0),
        Err(e) => return e,
    };
    let pos = list.elements.iter().position(|e| e.as_ref() == pivot);
    match pos {
        Some(i) => {
            let insert_at = if before { i } else { i + 1 };
            list.elements.insert(insert_at, value);
            let len = list.elements.len() as i64;
            if let Err(e) = store_list(session, id, &mut entry, list, key) {
                return e;
            }
            RedisReply::Integer(len)
        }
        None => RedisReply::Integer(-1),
    }
}

fn pop_side(list: &mut ListValue, left: bool) -> Option<Bytes> {
    if left {
        list.elements.pop_front()
    } else {
        list.elements.pop_back()
    }
}

fn push_side(list: &mut ListValue, value: Bytes, left: bool) {
    if left {
        list.elements.push_front(value);
    } else {
        list.elements.push_back(value);
    }
}

/// Load destination list (or create), push, store. `WRONGTYPE` when not a list.
fn push_into(
    session: &SessionState,
    id: ContainerId,
    dest: &str,
    value: Bytes,
    left: bool,
) -> Result<(), RedisReply> {
    let (mut entry, mut list) = match load_list(session, id, dest)? {
        Some(v) => v,
        None => (fresh_list_entry(dest), ListValue::default()),
    };
    push_side(&mut list, value, left);
    store_list(session, id, &mut entry, list, dest)
}

fn cmd_rpoplpush(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 2 {
        return render::arity("rpoplpush");
    }
    let (src, dest) = (args[0], args[1]);
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (mut entry, mut list) = match load_list(session, id, src) {
        Ok(Some(v)) => v,
        Ok(None) => return RedisReply::Bulk(None),
        Err(e) => return e,
    };
    let Some(value) = pop_side(&mut list, false) else {
        return RedisReply::Bulk(None);
    };
    if let Err(e) = store_list(session, id, &mut entry, list, src) {
        return e;
    }
    if let Err(e) = push_into(session, id, dest, value.clone(), true) {
        return e;
    }
    RedisReply::Bulk(Some(value.to_vec()))
}

fn cmd_lmove(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 4 {
        return render::arity("lmove");
    }
    let from_left = match args[2].to_ascii_uppercase().as_str() {
        "LEFT" => true,
        "RIGHT" => false,
        _ => return render::error("ERR syntax error"),
    };
    let to_left = match args[3].to_ascii_uppercase().as_str() {
        "LEFT" => true,
        "RIGHT" => false,
        _ => return render::error("ERR syntax error"),
    };
    let (src, dest) = (args[0], args[1]);
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let (mut entry, mut list) = match load_list(session, id, src) {
        Ok(Some(v)) => v,
        Ok(None) => return RedisReply::Bulk(None),
        Err(e) => return e,
    };
    let Some(value) = pop_side(&mut list, from_left) else {
        return RedisReply::Bulk(None);
    };
    if let Err(e) = store_list(session, id, &mut entry, list, src) {
        return e;
    }
    if let Err(e) = push_into(session, id, dest, value.clone(), to_left) {
        return e;
    }
    RedisReply::Bulk(Some(value.to_vec()))
}

/// `017` T027: immediate attempt for `BLPOP`/`BRPOP`. `Some` when an element was
/// available (or the key held the wrong type); `None` parks the connection.
pub fn try_block_pop(session: &mut SessionState, req: &BlockRequest) -> Option<RedisReply> {
    let left = matches!(req.kind, BlockKind::BlPop);
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return Some(e),
    };
    for k in &req.keys {
        let key = String::from_utf8_lossy(k).into_owned();
        let (mut entry, mut list) = match load_list(session, id, &key) {
            Ok(Some(v)) => v,
            Ok(None) => continue,
            Err(e) => return Some(e),
        };
        let Some(value) = pop_side(&mut list, left) else {
            continue;
        };
        if let Err(e) = store_list(session, id, &mut entry, list, &key) {
            return Some(e);
        }
        return Some(RedisReply::Array(vec![
            RedisReply::Bulk(Some(k.to_vec())),
            RedisReply::Bulk(Some(value.to_vec())),
        ]));
    }
    None
}

/// `017` T027: immediate attempt for `BRPOPLPUSH`/`BLMOVE`.
pub fn try_block_move(session: &mut SessionState, req: &BlockRequest) -> Option<RedisReply> {
    let src = String::from_utf8_lossy(&req.keys[0]).into_owned();
    let Some(dest) = req.dest.as_ref().map(|d| String::from_utf8_lossy(d).into_owned()) else {
        return None;
    };
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return Some(e),
    };
    let (mut entry, mut list) = match load_list(session, id, &src) {
        Ok(Some(v)) => v,
        Ok(None) => return None,
        Err(e) => return Some(e),
    };
    let Some(value) = pop_side(&mut list, req.from_left) else {
        return None;
    };
    if let Err(e) = store_list(session, id, &mut entry, list, &src) {
        return Some(e);
    }
    if let Err(e) = push_into(session, id, &dest, value.clone(), req.to_left) {
        return Some(e);
    }
    Some(RedisReply::Bulk(Some(value.to_vec())))
}

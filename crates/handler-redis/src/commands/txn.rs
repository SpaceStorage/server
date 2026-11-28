//! Transaction session (`017` data-model §6, US3 T044–T045). Connection-local,
//! Redis queued-and-atomic **no-rollback** semantics.
//!
//! Queued commands are stored as raw `argv` and **re-dispatched** through the
//! normal command path at `EXEC` time. The data-model's
//! `QueuedCommand.lowered: LogicalRequest` is not realisable for every verb:
//! the handler executes most commands by direct model access rather than by
//! building a `LogicalRequest`, so a faithful `EXEC` replays the stored
//! `argv`. The `lowered` field is retained for shape but is always `None`.
//!
//! `watched` maps key → `RedisEntry.version` captured at `WATCH` time, or
//! `None` when the key was absent (so an absent→present transition aborts).

use std::collections::BTreeMap;

use bytes::Bytes;
use spacestorage_compat::{ClassifyOutcome, ProtocolId, classify_outcome};
use spacestorage_query::LogicalRequest;

use crate::render;
use super::{RedisReply, SessionState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxnState {
    Idle,
    Queuing { dirty: bool },
    Executing,
}

/// A command queued for `EXEC`, with its already-lowered request.
#[derive(Debug)]
pub struct QueuedCommand {
    pub argv: Vec<Bytes>,
    /// Reserved: re-dispatch replays `argv` (see module docs), so this is
    /// always `None`.
    pub lowered: Option<LogicalRequest>,
}

/// `watched` maps key → `RedisEntry.version` captured at `WATCH` time (`None`
/// if the key was absent).
#[derive(Debug, Default)]
pub struct TransactionSession {
    pub state: Option<TxnState>,
    pub queue: Vec<QueuedCommand>,
    pub watched: BTreeMap<Bytes, Option<u64>>,
}

impl TransactionSession {
    pub fn state(&self) -> TxnState {
        self.state.unwrap_or(TxnState::Idle)
    }

    pub fn is_idle(&self) -> bool {
        matches!(self.state(), TxnState::Idle)
    }

    pub fn is_dirty(&self) -> bool {
        matches!(self.state(), TxnState::Queuing { dirty: true })
    }

    /// `MULTI`: begin a fresh queue. `WATCH` registrations are preserved.
    pub fn begin(&mut self) {
        self.state = Some(TxnState::Queuing { dirty: false });
        self.queue.clear();
    }

    /// Flag a queuing-time error; `EXEC` then aborts with `EXECABORT`.
    pub fn mark_dirty(&mut self) {
        self.state = Some(TxnState::Queuing { dirty: true });
    }

    /// Return to `Idle`, dropping the queue and every `WATCH`.
    pub fn reset(&mut self) {
        self.state = Some(TxnState::Idle);
        self.queue.clear();
        self.watched.clear();
    }
}

/// Commands that are never queued, so they reach [`dispatch`] while `Queuing`.
const NON_QUEUED: &[&str] = &["MULTI", "EXEC", "DISCARD", "WATCH", "UNWATCH", "RESET", "QUIT"];

/// Queue-time interception (`017` T045 step 1). Returns `Some` only while the
/// session is `Queuing` and `upper` is a command that should be queued; a valid
/// command is appended (`+QUEUED`) and an invalid one is refused **and** marks
/// the transaction dirty (Redis behaviour).
pub fn intercept(session: &mut SessionState, upper: &str, args: &[&str]) -> Option<RedisReply> {
    if !matches!(session.txn.state(), TxnState::Queuing { .. }) {
        return None;
    }
    if NON_QUEUED.contains(&upper) {
        return None;
    }
    if let Some(err) = queue_validate(session, upper, args) {
        session.txn.mark_dirty();
        return Some(err);
    }
    let mut argv = Vec::with_capacity(args.len() + 1);
    argv.push(Bytes::from(upper.as_bytes().to_vec()));
    for a in args {
        argv.push(Bytes::from(a.as_bytes().to_vec()));
    }
    session.txn.queue.push(QueuedCommand { argv, lowered: None });
    Some(RedisReply::Simple("QUEUED".into()))
}

/// The connection-scoped transaction verbs (`017` T045). Returns `None` for
/// every other command so the caller falls through to the data families.
pub fn dispatch(session: &mut SessionState, upper: &str, args: &[&str]) -> Option<RedisReply> {
    match upper {
        "MULTI" => Some(cmd_multi(session)),
        "EXEC" => Some(cmd_exec(session)),
        "DISCARD" => Some(cmd_discard(session)),
        "WATCH" => Some(cmd_watch(session, args)),
        "UNWATCH" => Some(cmd_unwatch(session)),
        _ => None,
    }
}

fn cmd_multi(session: &mut SessionState) -> RedisReply {
    match session.txn.state() {
        TxnState::Queuing { .. } => {
            session.txn.mark_dirty();
            render::error("ERR MULTI calls can not be nested")
        }
        TxnState::Executing => render::error("ERR MULTI calls can not be nested"),
        TxnState::Idle => {
            session.txn.begin();
            RedisReply::Ok
        }
    }
}

fn cmd_discard(session: &mut SessionState) -> RedisReply {
    if !matches!(session.txn.state(), TxnState::Queuing { .. }) {
        return render::error("ERR DISCARD without MULTI");
    }
    session.txn.reset();
    RedisReply::Ok
}

fn cmd_watch(session: &mut SessionState, args: &[&str]) -> RedisReply {
    if matches!(session.txn.state(), TxnState::Queuing { .. }) {
        session.txn.mark_dirty();
        return render::error("ERR WATCH inside MULTI is not allowed");
    }
    if args.is_empty() {
        return render::arity("watch");
    }
    for key in args {
        let version = match session.keyspace_id() {
            Ok(id) => session.load_entry(id, key).ok().flatten().map(|e| e.version),
            Err(_) => None,
        };
        session
            .txn
            .watched
            .insert(Bytes::from(key.as_bytes().to_vec()), version);
    }
    RedisReply::Ok
}

fn cmd_unwatch(session: &mut SessionState) -> RedisReply {
    session.txn.watched.clear();
    RedisReply::Ok
}

fn cmd_exec(session: &mut SessionState) -> RedisReply {
    if !matches!(session.txn.state(), TxnState::Queuing { .. }) {
        return render::error("ERR EXEC without MULTI");
    }
    if session.txn.is_dirty() {
        session.txn.reset();
        return render::execabort();
    }
    if !watch_ok(session) {
        // WATCH conflict: RESP2 null array; the queue is discarded, nothing runs.
        session.txn.reset();
        return RedisReply::NullArray;
    }

    let queued = std::mem::take(&mut session.txn.queue);
    session.txn.state = Some(TxnState::Executing);
    let mut replies = Vec::with_capacity(queued.len());
    for q in &queued {
        let cmd = String::from_utf8_lossy(&q.argv[0]).into_owned();
        let rest_owned: Vec<String> = q.argv[1..]
            .iter()
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .collect();
        let rest: Vec<&str> = rest_owned.iter().map(String::as_str).collect();
        // An execution error becomes one element; earlier writes stay, later
        // commands still run (no rollback).
        replies.push(super::dispatch(session, &cmd, &rest));
    }
    session.txn.state = Some(TxnState::Idle);
    session.txn.queue.clear();
    session.txn.watched.clear();
    RedisReply::Array(replies)
}

/// Every watched key must still hold the recorded version (absent ⇒ `None`).
fn watch_ok(session: &SessionState) -> bool {
    if session.txn.watched.is_empty() {
        return true;
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(_) => return false,
    };
    for (key, expected) in &session.txn.watched {
        let key_str = String::from_utf8_lossy(key).into_owned();
        let current = match session.load_entry(id, &key_str) {
            Ok(Some(e)) => Some(e.version),
            Ok(None) => None,
            Err(_) => return false,
        };
        if current != *expected {
            return false;
        }
    }
    true
}

/// Queue-time validation: unknown command, blocking/subscribe verb, or an
/// obvious arity error.
fn queue_validate(session: &SessionState, upper: &str, args: &[&str]) -> Option<RedisReply> {
    if matches!(
        classify_outcome(session.profile, ProtocolId::Redis, upper),
        ClassifyOutcome::MustNot(_)
    ) {
        return Some(render::unknown_command(upper));
    }
    if is_blocking(upper, args) || is_subscribe(upper) {
        return Some(render::error(format!(
            "ERR {upper} is not allowed in transactions"
        )));
    }
    if !arity_ok(upper, args.len()) {
        return Some(render::arity(upper));
    }
    None
}

fn is_blocking(upper: &str, args: &[&str]) -> bool {
    match upper {
        "BLPOP" | "BRPOP" | "BRPOPLPUSH" | "BLMOVE" | "BZPOPMIN" | "BZPOPMAX" => true,
        "XREAD" | "XREADGROUP" => args.iter().any(|a| a.eq_ignore_ascii_case("BLOCK")),
        _ => false,
    }
}

fn is_subscribe(upper: &str) -> bool {
    matches!(
        upper,
        "SUBSCRIBE" | "UNSUBSCRIBE" | "PSUBSCRIBE" | "PUNSUBSCRIBE" | "SSUBSCRIBE" | "SUNSUBSCRIBE"
    )
}

/// A modest arity table covering the complete-product command set; `n` is the
/// argument count **excluding** the command name. Commands absent from the
/// table are not arity-checked at queue time (they still fail inline at EXEC).
fn arity_ok(upper: &str, n: usize) -> bool {
    match upper {
        // Connection / transaction verbs.
        "MULTI" | "EXEC" | "DISCARD" | "UNWATCH" => n == 0,
        "WATCH" => n >= 1,
        // Strings + key expiry.
        "GET" | "TTL" | "PTTL" | "STRLEN" | "INCR" | "DECR" | "PERSIST" | "EXPIRETIME"
        | "PEXPIRETIME" | "GETDEL" | "TYPE" | "DBSIZE" | "XLEN" | "SCARD" | "SMEMBERS"
        | "HLEN" | "HKEYS" | "HVALS" | "LLEN" | "ZCARD" => n == 1,
        "SET" | "EXPIRE" | "PEXPIRE" | "EXPIREAT" | "PEXPIREAT" => n >= 2,
        "APPEND" | "INCRBY" | "DECRBY" | "INCRBYFLOAT" | "GETSET" | "SETNX" => n == 2,
        "GETRANGE" | "SETRANGE" | "SETEX" | "PSETEX" | "LSET" | "LREM" | "LTRIM" => n == 3,
        "DEL" | "EXISTS" | "UNLINK" | "TOUCH" | "MGET" => n >= 1,
        "MSET" => n >= 2 && n % 2 == 0,
        // Hash.
        "HGET" | "HSTRLEN" | "HEXISTS" => n == 2,
        "HSET" | "HMSET" => n >= 3 && n % 2 == 1,
        "HSETNX" | "HINCRBY" | "HINCRBYFLOAT" => n == 3,
        "HDEL" | "HMGET" => n >= 2,
        "HGETALL" => n == 1,
        // List.
        "LPUSH" | "RPUSH" | "LPUSHX" | "RPUSHX" => n >= 2,
        "LPOP" | "RPOP" => n >= 1,
        "LRANGE" => n == 3,
        "LINDEX" => n == 2,
        // Set.
        "SADD" | "SREM" => n >= 2,
        "SISMEMBER" => n == 2,
        "SPOP" => n >= 1,
        // Sorted set.
        "ZADD" => n >= 3,
        "ZSCORE" => n == 2,
        "ZRANGE" | "ZRANGEBYSCORE" => n >= 3,
        // Stream.
        "XADD" => n >= 3,
        "XRANGE" | "XREVRANGE" => n >= 3,
        "XDEL" => n >= 2,
        // Anything else: not checked here.
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arity_table_rejects_obvious_errors() {
        assert!(!arity_ok("GET", 0));
        assert!(arity_ok("GET", 1));
        assert!(!arity_ok("SET", 1));
        assert!(arity_ok("SET", 2));
        assert!(arity_ok("SET", 4));
        assert!(!arity_ok("DEL", 0));
        assert!(arity_ok("DEL", 1));
        assert!(!arity_ok("HGET", 1));
        assert!(arity_ok("HGET", 2));
        assert!(!arity_ok("HSET", 2));
        assert!(arity_ok("HSET", 3));
        assert!(arity_ok("HSET", 5));
        assert!(!arity_ok("HSET", 4));
        assert!(!arity_ok("LPUSH", 1));
        assert!(arity_ok("LPUSH", 2));
        assert!(!arity_ok("SADD", 1));
        assert!(!arity_ok("ZADD", 2));
        assert!(arity_ok("ZADD", 3));
        assert!(!arity_ok("EXPIRE", 1));
        assert!(arity_ok("EXPIRE", 2));
    }
}

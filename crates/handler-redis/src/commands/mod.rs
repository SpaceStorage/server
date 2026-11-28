//! Command dispatch for the first-binary Redis K/V MUST list and (from `017`)
//! the complete-product command surface. Submodules are created by `017` T002;
//! behavior lands per user story.

pub mod bitmap;
pub mod hash;
pub mod hll;
pub mod list;
pub mod pubsub;
pub mod server;
pub mod set;
pub mod ss;
pub mod strings;
pub mod stream;
pub mod txn;
pub mod zset;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use spacestorage_compat::{
    ClassifyOutcome, DialectProfile, LimitServices, PrincipalHold, ProtocolId, classify_outcome,
};
use spacestorage_query::{CancelToken, LogicalRequest, PlannerEngine, QueryOptions, QueryResult};
use spacestorage_types::{ContainerCatalog, ContainerId, L3Model, TypeError};

use crate::{lower, render};
use crate::block::BlockRegistry;
use crate::pubsub::PubSubBus;
use crate::session::{ConnectionId, Subscription};

pub const MUST_COMMANDS: &[&str] = &[
    "AUTH", "PING", "GET", "SET", "DEL", "EXISTS", "SCAN", "SELECT", "TTL", "EXPIRE", "PTTL",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedisReply {
    Ok,
    Pong,
    /// RESP2 simple string (`+...`), used by the `+QUEUED` receipt.
    Simple(String),
    Bulk(Option<Vec<u8>>),
    Integer(i64),
    Array(Vec<RedisReply>),
    /// Several independent frames written back-to-back: subscribe/unsubscribe
    /// confirmations and the subscribe-mode `PING` array.
    Frames(Vec<RedisReply>),
    /// RESP2 null array (`*-1`); used by `EXEC` when `WATCH` aborts.
    NullArray,
    Error(String),
}

#[derive(Debug, Default)]
pub struct TtlMap {
    /// (container_id, key) → absolute expiry
    expires: HashMap<(ContainerId, String), Instant>,
}

impl TtlMap {
    pub fn set(&mut self, id: ContainerId, key: &str, ttl: Duration) {
        self.expires
            .insert((id, key.to_string()), Instant::now() + ttl);
    }

    pub fn clear(&mut self, id: ContainerId, key: &str) {
        self.expires.remove(&(id, key.to_string()));
    }

    pub fn ttl_secs(&self, id: ContainerId, key: &str) -> i64 {
        match self.expires.get(&(id, key.to_string())) {
            Some(deadline) => {
                let now = Instant::now();
                if *deadline <= now {
                    -2
                } else {
                    deadline.duration_since(now).as_secs() as i64
                }
            }
            None => -1, // key exists or not decided by caller
        }
    }

    pub fn pttl_ms(&self, id: ContainerId, key: &str) -> i64 {
        match self.expires.get(&(id, key.to_string())) {
            Some(deadline) => {
                let now = Instant::now();
                if *deadline <= now {
                    -2
                } else {
                    deadline.duration_since(now).as_millis() as i64
                }
            }
            None => -1,
        }
    }

    pub fn expired(&mut self, id: ContainerId, key: &str) -> bool {
        if let Some(deadline) = self.expires.get(&(id, key.to_string())) {
            if *deadline <= Instant::now() {
                self.expires.remove(&(id, key.to_string()));
                return true;
            }
        }
        false
    }
}

pub struct SessionState {
    pub catalog: Arc<RwLock<ContainerCatalog>>,
    pub ttls: Arc<RwLock<TtlMap>>,
    pub authenticated: bool,
    pub namespace: String,
    pub container: String,
    pub password_ok: bool,
    /// Expected password for AUTH (demo default).
    pub expected_secret: String,
    pub expected_user: String,
    /// Effective configured limits + principal connection counters (FR-011).
    pub limit_services: LimitServices,
    /// Held while this connection is authenticated for a principal.
    pub principal_hold: Option<PrincipalHold>,
    pub tenancy: Option<Arc<spacestorage_tenancy::Tenancy>>,
    pub connection_quota: Option<spacestorage_tenancy::ConnectionQuotaHold>,
    pub principal: Option<String>,
    /// Active dialect profile (`017` T012). The complete-product table is
    /// consulted only when this is `CompleteProduct`.
    pub profile: DialectProfile,
    /// Shared blocking wake registry (`017` T027); every write wakes parked
    /// connections so a blocking command re-checks its keys.
    pub block_registry: Arc<BlockRegistry>,
    /// Per-stream-key total-order leadership lease (`017` T038). Shared across
    /// connections; the node (`006`) grants it. Default dev/harness single-node
    /// holder; a running node injects a `controlplane`-backed provider.
    pub stream_leases: Arc<dyn spacestorage_protocol_core::StreamLeaseProvider>,
    /// Connection-local transaction state (`017` US3, data-model §6).
    pub txn: txn::TransactionSession,
    /// This connection's RESP2 identity (`CLIENT ID`) — the pub/sub bus key.
    pub connection_id: ConnectionId,
    /// `Some` while in subscribe mode (`017` data-model §5).
    pub subscriptions: Option<Subscription>,
    /// Process-local, namespace-scoped pub/sub bus (`017` US3). Shared across
    /// every connection of one handler, never durable.
    pub pubsub: Arc<PubSubBus>,
    /// Handler-local `INFO` counters (`017` US4 T051/T055). Shared across every
    /// connection of one handler; records real connections/commands/hits/misses.
    pub stats: Arc<crate::stats::RedisStats>,
    /// Node facts for `INFO server`/`memory`/`replication` (`017` US4 T051).
    /// Defaults to the standalone provider; the running node injects real facts.
    pub node_info: Arc<dyn spacestorage_protocol_core::NodeInfoProvider>,
    /// Configured blocking cap (`017` US4): surfaced and updated by
    /// `CONFIG GET|SET max_block`; applied in `run_block`.
    pub max_block: Option<Duration>,
    /// This connection's `CLIENT SETNAME` label (`017` US4), if any.
    pub client_name: Option<String>,
}

impl SessionState {
    pub fn demo(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self {
            catalog,
            ttls: Arc::new(RwLock::new(TtlMap::default())),
            authenticated: false,
            namespace: "demo".into(),
            container: "redis".into(),
            password_ok: false,
            expected_secret: "demo".into(),
            expected_user: "demo".into(),
            limit_services: LimitServices::built_in(),
            principal_hold: None,
            tenancy: None,
            connection_quota: None,
            principal: None,
            profile: DialectProfile::FirstBinary,
            block_registry: Arc::new(BlockRegistry::new()),
            stream_leases: Arc::new(crate::lease::StreamLeaseAuthority::single_node()),
            txn: txn::TransactionSession::default(),
            connection_id: ConnectionId(0),
            subscriptions: None,
            pubsub: Arc::new(PubSubBus::new()),
            stats: Arc::new(crate::stats::RedisStats::new()),
            node_info: Arc::new(spacestorage_protocol_core::StandaloneNodeInfo::new()),
            max_block: None,
            client_name: None,
        }
    }

    /// `Some` subscriptions ⇒ subscribe mode (`017` US3): only the allowed
    /// pub/sub verbs, `PING`, `QUIT`, and `RESET` are served.
    pub fn subscribe_mode(&self) -> bool {
        self.subscriptions.is_some()
    }

    /// K/V MUST path: auto-create `K/V Store` when missing (T053 seam).
    #[allow(dead_code)]
    fn ensure_kv(&self) -> Result<ContainerId, RedisReply> {
        self.ensure_container(&[L3Model::KvStore], true)
    }

    /// Canonical blob path for Document Store — no auto-create (admin-create first).
    #[allow(dead_code)]
    fn ensure_blob(&self) -> Result<ContainerId, RedisReply> {
        self.ensure_container(&[L3Model::KvStore, L3Model::DocumentStore], false)
    }

    /// Data verbs: auto-create K/V; allow an admin-created Document Store as blob (T032).
    fn ensure_data(&self) -> Result<ContainerId, RedisReply> {
        self.ensure_container(&[L3Model::KvStore, L3Model::DocumentStore], true)
    }

    fn ensure_container(
        &self,
        allowed: &[L3Model],
        auto_create_kv: bool,
    ) -> Result<ContainerId, RedisReply> {
        // Metadata lookup only (a `describe`, not a data op): resolve the
        // existing key-space container without holding the lock across the IR.
        let existing = {
            let cat = self.catalog.read().expect("catalog");
            cat.describe(&self.namespace, &self.container)
                .map(|c| (c.id, c.model))
        };
        match existing {
            Ok((id, model)) => {
                if !allowed.contains(&model) {
                    return Err(RedisReply::Error(
                        "WRONGTYPE Operation against a key holding the wrong kind of value".into(),
                    ));
                }
                Ok(id)
            }
            Err(TypeError::NotFound) if auto_create_kv => {
                // Auto-create through the shared engine (`002` FR-035): the
                // `ensure_kv` TypeOp maps to the L3 `KvStore` model.
                self.planner_exec(LogicalRequest::TypeOp {
                    container: self.container.clone(),
                    op: "ensure_kv".into(),
                })?;
                let cat = self.catalog.read().expect("catalog");
                cat.describe(&self.namespace, &self.container)
                    .map(|c| c.id)
                    .map_err(|e| RedisReply::Error(format!("ERR {e:?}")))
            }
            Err(TypeError::NotFound) => Err(RedisReply::Error(
                "ERR no such container (admin-create Document Store / K/V first)".into(),
            )),
            Err(e) => Err(RedisReply::Error(format!("ERR {e:?}"))),
        }
    }

    fn purge_if_expired(&self, id: ContainerId, key: &str) {
        let expired = {
            let mut ttls = self.ttls.write().expect("ttl");
            ttls.expired(id, key)
        };
        if expired {
            let _ = self.delete_entry(id, key);
            // `017` US4: a purge-on-read is a real expired-key deletion.
            self.stats.record_expired();
        }
    }

    fn planner_exec(&self, req: LogicalRequest) -> Result<QueryResult, RedisReply> {
        let mut engine = PlannerEngine::new(Arc::clone(&self.catalog));
        if let Some(t) = &self.tenancy {
            engine = engine.with_tenancy(Arc::clone(t));
        }
        let mut options = QueryOptions::default();
        if let Some(login) = self.principal.as_deref() {
            options.principal = Some(login.to_string());
        }
        engine
            .execute_blocking(
                &self.namespace,
                "redis",
                "redis",
                req,
                options,
                CancelToken::new(),
            )
            .map_err(|e| RedisReply::Error(format!("ERR {e}")))
    }

    /// Complete-product (`017`): the key-space container id, auto-created on
    /// first use (`002` auto-create / T030).
    pub(crate) fn keyspace_id(&self) -> Result<ContainerId, RedisReply> {
        self.ensure_data()
    }

    /// Read + decode one canonical entry. Expired keys purge-on-read and appear
    /// absent (data-model §7). Raw (non-canonical) blobs degrade to a string.
    pub(crate) fn load_entry(
        &self,
        id: ContainerId,
        key: &str,
    ) -> Result<Option<lower::RedisEntry>, RedisReply> {
        let bytes = match self
            .planner_exec(LogicalRequest::KeyGet {
                container: self.container.clone(),
                key: key.to_string(),
            })?
            .blob
        {
            Some(v) => v,
            None => return Ok(None),
        };
        let key_bytes = bytes::Bytes::from(key.as_bytes().to_vec());
        let mut entry = match lower::decode_entry(key_bytes, &bytes) {
            Some(e) => e,
            None => lower::RedisEntry::new(
                bytes::Bytes::from(key.as_bytes().to_vec()),
                lower::EntryPayload::Bytes(bytes),
            ),
        };
        if let Some(d) = entry.expire_at {
            if d.is_past(crate::expiry::TtlInstant::now()) {
                let _ = self.delete_entry(id, key)?;
                // `017` US4: an expired key purged on read is counted for `INFO stats`.
                self.stats.record_expired();
                return Ok(None);
            }
        }
        entry.key = bytes::Bytes::from(key.as_bytes().to_vec());
        Ok(Some(entry))
    }

    /// Write one canonical entry (durable hook runs for Persistent/Hybrid).
    ///
    /// Data-model §2: `version` is bumped on every mutation so `WATCH` can
    /// detect a concurrent write. A first write keeps the incoming version; an
    /// overwrite advances the stored one.
    pub(crate) fn store_entry(
        &self,
        id: ContainerId,
        entry: &lower::RedisEntry,
    ) -> Result<(), RedisReply> {
        let key = String::from_utf8_lossy(&entry.key).into_owned();
        let mut stored = entry.clone();
        if let Some(prev) = self.load_entry(id, &key)? {
            stored.version = prev.version.wrapping_add(1);
        }
        let blob = lower::encode_entry(&stored);
        self.planner_exec(LogicalRequest::Object {
            container: self.container.clone(),
            key,
            bytes: blob,
        })?;
        self.block_registry.wake();
        Ok(())
    }

    pub(crate) fn delete_entry(
        &self,
        _id: ContainerId,
        key: &str,
    ) -> Result<bool, RedisReply> {
        let r = self.planner_exec(LogicalRequest::KeyDelete {
            container: self.container.clone(),
            key: key.to_string(),
        })?;
        self.block_registry.wake();
        Ok(r.tag == "DELETE 1")
    }

    pub(crate) fn entry_exists(
        &self,
        id: ContainerId,
        key: &str,
    ) -> Result<bool, RedisReply> {
        Ok(self.load_entry(id, key)?.is_some())
    }
}

pub fn dispatch(session: &mut SessionState, cmd: &str, args: &[&str]) -> RedisReply {
    // `017` US4: every dispatched command is counted once with its measured
    // wall-clock time — the top-level command and each `EXEC`-replayed
    // sub-command (which re-enters through this same seam).
    let started = Instant::now();
    let reply = dispatch_inner(session, cmd, args);
    session
        .stats
        .record_command(cmd, started.elapsed().as_micros() as u64);
    reply
}

fn dispatch_inner(session: &mut SessionState, cmd: &str, args: &[&str]) -> RedisReply {
    let upper = cmd.to_ascii_uppercase();

    // `017` US3: queue-time interception runs before classification so that a
    // queuing-time error (unknown command, arity, blocking/subscribe verb) is
    // reported against the offending command and marks the transaction dirty.
    if session.profile == DialectProfile::CompleteProduct {
        if let Some(reply) = txn::intercept(session, &upper, args) {
            return reply;
        }
    }

    // `017` US4: `OBJECT`/`CONFIG` are two-word commands whose capability row is
    // keyed on the pair (`OBJECT ENCODING`, `CONFIG GET`), not the bare verb.
    // When the bare token is not `Must`, retry with `<VERB> <ARG>` and keep the
    // bare verdict if the pair is not `Must` either (the capability table is
    // unchanged; this only closes the lookup gap).
    let outcome = match classify_outcome(session.profile, ProtocolId::Redis, &upper) {
        ClassifyOutcome::Must => ClassifyOutcome::Must,
        other => match args.first() {
            Some(arg) => {
                let two_word = format!("{upper} {}", arg.to_ascii_uppercase());
                match classify_outcome(session.profile, ProtocolId::Redis, &two_word) {
                    ClassifyOutcome::Must => ClassifyOutcome::Must,
                    _ => other,
                }
            }
            None => other,
        },
    };
    match outcome {
        ClassifyOutcome::Must => {}
        ClassifyOutcome::MustNot(_) => {
            return RedisReply::Error(format!("ERR unknown command '{cmd}'"));
        }
    }

    if upper != "AUTH" && upper != "PING" && !session.authenticated {
        // Allow PING before AUTH (common health); AUTH required for data verbs.
        if upper != "PING" {
            return RedisReply::Error("NOAUTH Authentication required.".into());
        }
    }

    // `017` US3: while subscribed, only (P|S)SUBSCRIBE / (P|S)UNSUBSCRIBE /
    // PING / QUIT / RESET are allowed. PING answers with the array form.
    if session.subscribe_mode() {
        match upper.as_str() {
            "SUBSCRIBE" | "UNSUBSCRIBE" | "PSUBSCRIBE" | "PUNSUBSCRIBE" | "SSUBSCRIBE"
            | "SUNSUBSCRIBE" => {}
            "PING" => return ping_in_subscribe_mode(args),
            "QUIT" | "RESET" => {}
            _ => {
                return RedisReply::Error(format!(
                    "ERR Can't execute '{}': only (P|S)SUBSCRIBE / (P|S)UNSUBSCRIBE / PING / QUIT / RESET are allowed in this context",
                    cmd.to_ascii_lowercase()
                ));
            }
        }
    }

    match upper.as_str() {
        "AUTH" => {
            let (user, pass) = match args {
                [p] => (session.expected_user.as_str(), *p),
                [u, p] => (*u, *p),
                _ => return RedisReply::Error("ERR wrong number of arguments for 'auth'".into()),
            };
            if user == session.expected_user && pass == session.expected_secret {
                if session.connection_quota.is_none() {
                    match spacestorage_tenancy::bind_namespace_session(
                        session.tenancy.as_ref(),
                        user,
                        session.namespace.as_str(),
                    ) {
                        Ok(hold) => session.connection_quota = hold,
                        Err(err) => return RedisReply::Error(format!("ERR {err}")),
                    }
                }
                // FR-011: count authenticated session per principal; reject without waiting.
                if session.principal_hold.is_none() {
                    match session.limit_services.try_hold_principal(user) {
                        Ok(hold) => session.principal_hold = Some(hold),
                        Err(e) => {
                            return RedisReply::Error(format!("ERR max connections {e}"));
                        }
                    }
                }
                session.principal = Some(user.to_string());
                session.authenticated = true;
                session.password_ok = true;
                RedisReply::Ok
            } else {
                RedisReply::Error("WRONGPASS invalid username-password pair".into())
            }
        }
        "PING" => match args {
            [] => RedisReply::Pong,
            [msg] => RedisReply::Bulk(Some(msg.as_bytes().to_vec())),
            _ => RedisReply::Error("ERR wrong number of arguments for 'ping'".into()),
        },
        "SELECT" => {
            // `016` first-binary: no-op inside the bound namespace (index ignored).
            // `017` complete product: the dialect binds one namespace per
            // credential (`002`), so only index 0 (already bound) is a no-op;
            // `SELECT n>0` is the documented not-supported exception
            // (contracts/not-supported.md §11).
            if session.profile == DialectProfile::CompleteProduct {
                match args.first().copied() {
                    Some("0") => RedisReply::Ok,
                    Some(_) => RedisReply::Error(
                        "ERR SELECT is not supported: namespaces are bound to credentials".into(),
                    ),
                    None => render::arity("select"),
                }
            } else {
                RedisReply::Ok
            }
        }
        _ => {
            if session.profile == DialectProfile::CompleteProduct {
                complete_dispatch(session, &upper, args)
            } else {
                first_binary_dispatch(session, &upper, args)
            }
        }
    }
}

/// Subscribe-mode `PING` (`017` US3): a single `[pong, message]` array frame.
fn ping_in_subscribe_mode(args: &[&str]) -> RedisReply {
    let payload = match args {
        [] => Vec::new(),
        [m] => m.as_bytes().to_vec(),
        _ => return render::arity("ping"),
    };
    RedisReply::Frames(vec![RedisReply::Array(vec![
        RedisReply::Bulk(Some(b"pong".to_vec())),
        RedisReply::Bulk(Some(payload)),
    ])])
}

/// `017` T012: the complete-product dispatch seam. Classification already ran
/// in [`dispatch`]; here in-table commands lower onto the shared key-space entry
/// model. Returns a protocol error (never `+OK`) for an unhandled verb.
fn complete_dispatch(session: &mut SessionState, upper: &str, args: &[&str]) -> RedisReply {
    if let Some(reply) = txn::dispatch(session, upper, args) {
        return reply;
    }
    if let Some(reply) = pubsub::dispatch(session, upper, args) {
        return reply;
    }
    if let Some(reply) = server::dispatch(session, upper, args) {
        return reply;
    }
    if let Some(reply) = strings::dispatch(session, upper, args) {
        return reply;
    }
    if let Some(reply) = hash::dispatch(session, upper, args) {
        return reply;
    }
    if let Some(reply) = list::dispatch(session, upper, args) {
        return reply;
    }
    if let Some(reply) = set::dispatch(session, upper, args) {
        return reply;
    }
    if let Some(reply) = zset::dispatch(session, upper, args) {
        return reply;
    }
    if let Some(reply) = bitmap::dispatch(session, upper, args) {
        return reply;
    }
    if let Some(reply) = hll::dispatch(session, upper, args) {
        return reply;
    }
    if let Some(reply) = stream::dispatch(session, upper, args) {
        return reply;
    }
    render::unknown_command(upper)
}

/// `016` first-binary K/V string subset (unchanged).
fn first_binary_dispatch(session: &mut SessionState, upper: &str, args: &[&str]) -> RedisReply {
    match upper {
        "SET" => {
            let Some(key) = args.first() else {
                return RedisReply::Error("ERR wrong number of arguments for 'set'".into());
            };
            let Some(val) = args.get(1) else {
                return RedisReply::Error("ERR wrong number of arguments for 'set'".into());
            };
            let limits = session.limit_services.limits();
            if let Err(e) = limits.check_key(key.len() as u64) {
                return RedisReply::Error(format!("ERR {e}"));
            }
            if let Err(e) = limits.check_value(val.len() as u64) {
                return RedisReply::Error(format!("ERR {e}"));
            }
            let id = match session.ensure_data() {
                Ok(id) => id,
                Err(e) => return e,
            };
            // Optional EX seconds
            let mut ex: Option<u64> = None;
            let mut i = 2;
            while i < args.len() {
                let opt = args[i].to_ascii_uppercase();
                if opt == "EX" {
                    if let Some(n) = args.get(i + 1).and_then(|s| s.parse().ok()) {
                        ex = Some(n);
                        i += 2;
                        continue;
                    }
                }
                i += 1;
            }
            if let Err(e) = session.planner_exec(LogicalRequest::Object {
                container: session.container.clone(),
                key: (*key).into(),
                bytes: val.as_bytes().to_vec(),
            }) {
                return e;
            }
            if let Some(secs) = ex {
                session
                    .ttls
                    .write()
                    .expect("ttl")
                    .set(id, key, Duration::from_secs(secs));
            } else {
                session.ttls.write().expect("ttl").clear(id, key);
            }
            RedisReply::Ok
        }
        "GET" => {
            let Some(key) = args.first() else {
                return RedisReply::Error("ERR wrong number of arguments for 'get'".into());
            };
            let limits = session.limit_services.limits();
            if let Err(e) = limits.check_key(key.len() as u64) {
                return RedisReply::Error(format!("ERR {e}"));
            }
            let id = match session.ensure_data() {
                Ok(id) => id,
                Err(e) => return e,
            };
            session.purge_if_expired(id, key);
            match session.planner_exec(LogicalRequest::KeyGet {
                container: session.container.clone(),
                key: (*key).into(),
            }) {
                Ok(r) => match r.blob {
                    Some(bytes) => {
                        if let Err(e) = limits.check_result(bytes.len() as u64) {
                            RedisReply::Error(format!("ERR {e}"))
                        } else {
                            RedisReply::Bulk(Some(bytes))
                        }
                    }
                    None => RedisReply::Bulk(None),
                },
                Err(e) => e,
            }
        }
        "DEL" => {
            if args.is_empty() {
                return RedisReply::Error("ERR wrong number of arguments for 'del'".into());
            }
            let id = match session.ensure_data() {
                Ok(id) => id,
                Err(e) => return e,
            };
            let mut n = 0i64;
            let mut ttls = session.ttls.write().expect("ttl");
            for key in args {
                match session.planner_exec(LogicalRequest::KeyDelete {
                    container: session.container.clone(),
                    key: (*key).into(),
                }) {
                    Ok(r) if r.tag == "DELETE 1" => {
                        ttls.clear(id, key);
                        n += 1;
                    }
                    Ok(_) => {}
                    Err(e) => return e,
                }
            }
            RedisReply::Integer(n)
        }
        "EXISTS" => {
            let id = match session.ensure_data() {
                Ok(id) => id,
                Err(e) => return e,
            };
            let mut n = 0i64;
            for key in args {
                session.purge_if_expired(id, key);
                match session.entry_exists(id, key) {
                    Ok(true) => n += 1,
                    Ok(false) => {}
                    Err(e) => return e,
                }
            }
            RedisReply::Integer(n)
        }
        "SCAN" => {
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
            let _id = match session.ensure_data() {
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
        "EXPIRE" => {
            let Some(key) = args.first() else {
                return RedisReply::Error("ERR wrong number of arguments for 'expire'".into());
            };
            let Some(secs) = args.get(1).and_then(|s| s.parse::<u64>().ok()) else {
                return RedisReply::Error("ERR wrong number of arguments for 'expire'".into());
            };
            let id = match session.ensure_data() {
                Ok(id) => id,
                Err(e) => return e,
            };
            match session.entry_exists(id, key) {
                Ok(true) => {
                    session
                        .ttls
                        .write()
                        .expect("ttl")
                        .set(id, key, Duration::from_secs(secs));
                    RedisReply::Integer(1)
                }
                Ok(false) => RedisReply::Integer(0),
                Err(e) => e,
            }
        }
        "TTL" => {
            let Some(key) = args.first() else {
                return RedisReply::Error("ERR wrong number of arguments for 'ttl'".into());
            };
            let id = match session.ensure_data() {
                Ok(id) => id,
                Err(e) => return e,
            };
            session.purge_if_expired(id, key);
            match session.entry_exists(id, key) {
                Ok(false) => RedisReply::Integer(-2),
                Ok(true) => {
                    let t = session.ttls.read().expect("ttl").ttl_secs(id, key);
                    RedisReply::Integer(if t == -1 { -1 } else { t })
                }
                Err(e) => e,
            }
        }
        "PTTL" => {
            let Some(key) = args.first() else {
                return RedisReply::Error("ERR wrong number of arguments for 'pttl'".into());
            };
            let id = match session.ensure_data() {
                Ok(id) => id,
                Err(e) => return e,
            };
            session.purge_if_expired(id, key);
            match session.entry_exists(id, key) {
                Ok(false) => RedisReply::Integer(-2),
                Ok(true) => {
                    let t = session.ttls.read().expect("ttl").pttl_ms(id, key);
                    RedisReply::Integer(if t == -1 { -1 } else { t })
                }
                Err(e) => e,
            }
        }
        _ => RedisReply::Error(format!("ERR unknown command '{upper}'")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spacestorage_types::StorageModeChoice;

    #[test]
    fn set_rejects_oversized_key() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(cat);
        s.authenticated = true;
        let big = "k".repeat(spacestorage_compat::DEFAULT_MAX_KEY as usize + 1);
        match dispatch(&mut s, "SET", &[&big, "v"]) {
            RedisReply::Error(e) => assert!(e.contains("limit_exceeded"), "{e}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn auth_rejects_over_principal_cap() {
        use spacestorage_compat::{LimitServices, Limits, SessionCounters};
        let counters = Arc::new(SessionCounters::new());
        let mut lim = Limits::default();
        lim.max_connections_per_principal = 1;
        let svc = LimitServices::with_fixed(Arc::clone(&counters), lim);
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut a = SessionState::demo(Arc::clone(&cat));
        a.limit_services = svc.clone();
        let mut b = SessionState::demo(cat);
        b.limit_services = svc;
        assert!(matches!(
            dispatch(&mut a, "AUTH", &["demo", "demo"]),
            RedisReply::Ok
        ));
        match dispatch(&mut b, "AUTH", &["demo", "demo"]) {
            RedisReply::Error(e) => {
                assert!(e.contains("max connections"), "{e}");
                assert!(e.contains("limit_exceeded"), "{e}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn auth_refuses_replication_and_unbound_login() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut replication = SessionState::demo(Arc::clone(&cat));
        replication.expected_user = "replication".into();
        replication.expected_secret = "secret".into();
        match dispatch(&mut replication, "AUTH", &["replication", "secret"]) {
            RedisReply::Error(e) => assert!(e.contains("ReplicationNotTenant"), "{e}"),
            other => panic!("{other:?}"),
        }
        assert!(!replication.authenticated);

        let mut unbound = SessionState::demo(cat);
        unbound.expected_user = "reader".into();
        unbound.expected_secret = "secret".into();
        unbound.namespace.clear();
        match dispatch(&mut unbound, "AUTH", &["reader", "secret"]) {
            RedisReply::Error(e) => assert!(e.contains("AccessDenied"), "{e}"),
            other => panic!("{other:?}"),
        }
        assert!(!unbound.authenticated);
    }

    #[test]
    fn set_uses_configured_max_key() {
        use spacestorage_compat::{LimitServices, Limits, SessionCounters};
        let mut lim = Limits::default();
        lim.max_key = 4;
        let svc = LimitServices::with_fixed(Arc::new(SessionCounters::new()), lim);
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(cat);
        s.limit_services = svc;
        s.authenticated = true;
        match dispatch(&mut s, "SET", &["abcde", "v"]) {
            RedisReply::Error(e) => assert!(e.contains("limit_exceeded"), "{e}"),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            dispatch(&mut s, "SET", &["abcd", "v"]),
            RedisReply::Ok
        ));
    }

    #[test]
    fn must_list_and_off_list() {
        let catalog = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(catalog);
        assert!(matches!(
            dispatch(&mut s, "AUTH", &["demo", "demo"]),
            RedisReply::Ok
        ));
        assert!(matches!(dispatch(&mut s, "PING", &[]), RedisReply::Pong));
        assert!(matches!(
            dispatch(&mut s, "SET", &["k", "v"]),
            RedisReply::Ok
        ));
        assert!(matches!(
            dispatch(&mut s, "GET", &["k"]),
            RedisReply::Bulk(Some(_))
        ));
        assert!(matches!(
            dispatch(&mut s, "EXISTS", &["k"]),
            RedisReply::Integer(1)
        ));
        assert!(matches!(dispatch(&mut s, "SELECT", &["0"]), RedisReply::Ok));
        assert!(matches!(
            dispatch(&mut s, "EXPIRE", &["k", "60"]),
            RedisReply::Integer(1)
        ));
        assert!(matches!(
            dispatch(&mut s, "TTL", &["k"]),
            RedisReply::Integer(_)
        ));
        assert!(matches!(
            dispatch(&mut s, "HGET", &["k", "f"]),
            RedisReply::Error(_)
        ));
        assert!(matches!(
            dispatch(&mut s, "JSON.GET", &["k"]),
            RedisReply::Error(_)
        ));
    }

    #[test]
    fn document_store_admin_create_blob_crud() {
        let catalog = Arc::new(RwLock::new(ContainerCatalog::new()));
        catalog
            .write()
            .unwrap()
            .create(
                "demo",
                "docs",
                L3Model::DocumentStore,
                false,
                None,
                StorageModeChoice::Persistent,
            )
            .expect("admin-create Document Store");
        let mut s = SessionState::demo(Arc::clone(&catalog));
        s.container = "docs".into();
        assert!(matches!(
            dispatch(&mut s, "AUTH", &["demo", "demo"]),
            RedisReply::Ok
        ));
        assert!(matches!(
            dispatch(&mut s, "SET", &["doc1", r#"{"a":1}"#]),
            RedisReply::Ok
        ));
        match dispatch(&mut s, "GET", &["doc1"]) {
            RedisReply::Bulk(Some(v)) => assert_eq!(v, br#"{"a":1}"#),
            other => panic!("expected blob GET, got {other:?}"),
        }
        assert!(matches!(
            dispatch(&mut s, "DEL", &["doc1"]),
            RedisReply::Integer(1)
        ));
        assert!(matches!(
            dispatch(&mut s, "JSON.GET", &["doc1"]),
            RedisReply::Error(_)
        ));
        // ensure_blob: missing Document Store refuses without auto-create as Document Store
        s.container = "missing-docs".into();
        assert!(matches!(
            s.ensure_blob(),
            Err(RedisReply::Error(e)) if e.contains("admin-create")
        ));
    }
}

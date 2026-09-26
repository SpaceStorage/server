//! Command dispatch for first-binary Redis K/V MUST list.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use spacestorage_compat::{classify_outcome, ClassifyOutcome, DialectProfile, ProtocolId};
use spacestorage_types::{
    ContainerCatalog, ContainerId, L3Model, StorageModeChoice, TypeError,
};

pub const MUST_COMMANDS: &[&str] = &[
    "AUTH", "PING", "GET", "SET", "DEL", "EXISTS", "SCAN", "SELECT", "TTL", "EXPIRE", "PTTL",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedisReply {
    Ok,
    Pong,
    Bulk(Option<Vec<u8>>),
    Integer(i64),
    Array(Vec<RedisReply>),
    Error(String),
}

#[derive(Debug, Default)]
pub struct TtlMap {
    /// (container_id, key) → absolute expiry
    expires: HashMap<(ContainerId, String), Instant>,
}

impl TtlMap {
    pub fn set(&mut self, id: ContainerId, key: &str, ttl: Duration) {
        self.expires.insert((id, key.to_string()), Instant::now() + ttl);
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
        }
    }

    fn ensure_kv(&self) -> Result<ContainerId, RedisReply> {
        let mut cat = self.catalog.write().expect("catalog");
        match cat.describe(&self.namespace, &self.container) {
            Ok(c) => {
                if c.model != L3Model::KvStore {
                    return Err(RedisReply::Error(
                        "WRONGTYPE Operation against a key holding the wrong kind of value".into(),
                    ));
                }
                Ok(c.id)
            }
            Err(TypeError::NotFound) => cat
                .create(
                    &self.namespace,
                    &self.container,
                    L3Model::KvStore,
                    false,
                    None,
                    StorageModeChoice::Persistent,
                )
                .map_err(|e| RedisReply::Error(format!("ERR {e:?}"))),
            Err(e) => Err(RedisReply::Error(format!("ERR {e:?}"))),
        }
    }

    fn purge_if_expired(&self, id: ContainerId, key: &str) {
        let mut ttls = self.ttls.write().expect("ttl");
        if ttls.expired(id, key) {
            let mut cat = self.catalog.write().expect("catalog");
            let _ = cat.delete_row(id, key);
        }
    }
}

pub fn dispatch(session: &mut SessionState, cmd: &str, args: &[&str]) -> RedisReply {
    let upper = cmd.to_ascii_uppercase();
    match classify_outcome(DialectProfile::FirstBinary, ProtocolId::Redis, &upper) {
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

    match upper.as_str() {
        "AUTH" => {
            let (user, pass) = match args {
                [p] => (session.expected_user.as_str(), *p),
                [u, p] => (*u, *p),
                _ => return RedisReply::Error("ERR wrong number of arguments for 'auth'".into()),
            };
            if user == session.expected_user && pass == session.expected_secret {
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
            // First-binary: no-op inside bound namespace (index ignored when 0; others still no-op per dialect).
            RedisReply::Ok
        }
        "SET" => {
            let Some(key) = args.first() else {
                return RedisReply::Error("ERR wrong number of arguments for 'set'".into());
            };
            let Some(val) = args.get(1) else {
                return RedisReply::Error("ERR wrong number of arguments for 'set'".into());
            };
            let id = match session.ensure_kv() {
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
            {
                let mut cat = session.catalog.write().expect("catalog");
                if let Err(e) = cat.put(id, key, val.as_bytes().to_vec()) {
                    return RedisReply::Error(format!("ERR {e:?}"));
                }
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
            let id = match session.ensure_kv() {
                Ok(id) => id,
                Err(e) => return e,
            };
            session.purge_if_expired(id, key);
            let cat = session.catalog.read().expect("catalog");
            match cat.get(id, key) {
                Ok(Some(v)) => RedisReply::Bulk(Some(v.to_vec())),
                Ok(None) => RedisReply::Bulk(None),
                Err(e) => RedisReply::Error(format!("ERR {e:?}")),
            }
        }
        "DEL" => {
            if args.is_empty() {
                return RedisReply::Error("ERR wrong number of arguments for 'del'".into());
            }
            let id = match session.ensure_kv() {
                Ok(id) => id,
                Err(e) => return e,
            };
            let mut n = 0i64;
            let mut cat = session.catalog.write().expect("catalog");
            let mut ttls = session.ttls.write().expect("ttl");
            for key in args {
                match cat.delete_row(id, key) {
                    Ok(true) => {
                        ttls.clear(id, key);
                        n += 1;
                    }
                    Ok(false) => {}
                    Err(e) => return RedisReply::Error(format!("ERR {e:?}")),
                }
            }
            RedisReply::Integer(n)
        }
        "EXISTS" => {
            let id = match session.ensure_kv() {
                Ok(id) => id,
                Err(e) => return e,
            };
            let mut n = 0i64;
            for key in args {
                session.purge_if_expired(id, key);
                let cat = session.catalog.read().expect("catalog");
                match cat.exists(id, key) {
                    Ok(true) => n += 1,
                    Ok(false) => {}
                    Err(e) => return RedisReply::Error(format!("ERR {e:?}")),
                }
            }
            RedisReply::Integer(n)
        }
        "SCAN" => {
            let cursor: u64 = args
                .first()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let mut count = 10usize;
            let mut pattern: Option<&str> = None;
            let mut i = 1;
            while i < args.len() {
                let opt = args[i].to_ascii_uppercase();
                if opt == "COUNT" {
                    if let Some(n) = args.get(i + 1).and_then(|s| s.parse().ok()) {
                        count = n;
                    }
                    i += 2;
                } else if opt == "MATCH" {
                    pattern = args.get(i + 1).copied();
                    i += 2;
                } else {
                    i += 1;
                }
            }
            let id = match session.ensure_kv() {
                Ok(id) => id,
                Err(e) => return e,
            };
            let cat = session.catalog.read().expect("catalog");
            match cat.scan(id, cursor, count, pattern) {
                Ok((next, keys)) => RedisReply::Array(vec![
                    RedisReply::Bulk(Some(next.to_string().into_bytes())),
                    RedisReply::Array(
                        keys.into_iter()
                            .map(|k| RedisReply::Bulk(Some(k.into_bytes())))
                            .collect(),
                    ),
                ]),
                Err(e) => RedisReply::Error(format!("ERR {e:?}")),
            }
        }
        "EXPIRE" => {
            let Some(key) = args.first() else {
                return RedisReply::Error("ERR wrong number of arguments for 'expire'".into());
            };
            let Some(secs) = args.get(1).and_then(|s| s.parse::<u64>().ok()) else {
                return RedisReply::Error("ERR wrong number of arguments for 'expire'".into());
            };
            let id = match session.ensure_kv() {
                Ok(id) => id,
                Err(e) => return e,
            };
            let cat = session.catalog.read().expect("catalog");
            match cat.exists(id, key) {
                Ok(true) => {
                    drop(cat);
                    session
                        .ttls
                        .write()
                        .expect("ttl")
                        .set(id, key, Duration::from_secs(secs));
                    RedisReply::Integer(1)
                }
                Ok(false) => RedisReply::Integer(0),
                Err(e) => RedisReply::Error(format!("ERR {e:?}")),
            }
        }
        "TTL" => {
            let Some(key) = args.first() else {
                return RedisReply::Error("ERR wrong number of arguments for 'ttl'".into());
            };
            let id = match session.ensure_kv() {
                Ok(id) => id,
                Err(e) => return e,
            };
            session.purge_if_expired(id, key);
            let cat = session.catalog.read().expect("catalog");
            match cat.exists(id, key) {
                Ok(false) => RedisReply::Integer(-2),
                Ok(true) => {
                    let t = session.ttls.read().expect("ttl").ttl_secs(id, key);
                    RedisReply::Integer(if t == -1 { -1 } else { t })
                }
                Err(e) => RedisReply::Error(format!("ERR {e:?}")),
            }
        }
        "PTTL" => {
            let Some(key) = args.first() else {
                return RedisReply::Error("ERR wrong number of arguments for 'pttl'".into());
            };
            let id = match session.ensure_kv() {
                Ok(id) => id,
                Err(e) => return e,
            };
            session.purge_if_expired(id, key);
            let cat = session.catalog.read().expect("catalog");
            match cat.exists(id, key) {
                Ok(false) => RedisReply::Integer(-2),
                Ok(true) => {
                    let t = session.ttls.read().expect("ttl").pttl_ms(id, key);
                    RedisReply::Integer(if t == -1 { -1 } else { t })
                }
                Err(e) => RedisReply::Error(format!("ERR {e:?}")),
            }
        }
        _ => RedisReply::Error(format!("ERR unknown command '{cmd}'")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn must_list_and_off_list() {
        let catalog = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(catalog);
        assert!(matches!(dispatch(&mut s, "AUTH", &["demo", "demo"]), RedisReply::Ok));
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
        assert!(matches!(
            dispatch(&mut s, "SELECT", &["0"]),
            RedisReply::Ok
        ));
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
}

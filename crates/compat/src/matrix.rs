//! MUST / MUST NOT compatibility matrix (contracts/matrix.md).
//!
//! Handlers MUST call [`classify`] (or the protocol helpers) **before**
//! lowering to `LogicalRequest`. `MustNot` → protocol ErrorRenderer; no IR.

use crate::error::CompatError;
use crate::profile::DialectProfile;
use crate::wire::ProtocolId;
use serde::{Deserialize, Serialize};

/// Verb class relative to the active dialect profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VerbClass {
    Must,
    MustNot,
}

/// Result of classify with the preferred refuse code for MustNot verbs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassifyOutcome {
    Must,
    MustNot(CompatError),
}

impl ClassifyOutcome {
    pub fn class(&self) -> VerbClass {
        match self {
            Self::Must => VerbClass::Must,
            Self::MustNot(_) => VerbClass::MustNot,
        }
    }

    pub fn is_must(&self) -> bool {
        matches!(self, Self::Must)
    }

    pub fn into_result(self) -> Result<(), CompatError> {
        match self {
            Self::Must => Ok(()),
            Self::MustNot(e) => Err(e),
        }
    }
}

fn normalize_verb(verb: &str) -> String {
    verb.trim().to_ascii_uppercase().replace('-', "_")
}

fn generic_must_not(protocol: ProtocolId, verb: &str) -> CompatError {
    CompatError::CompatMustNot {
        protocol: protocol.as_str().to_string(),
        verb: verb.to_string(),
    }
}

/// Classify a verb for `(profile, protocol)`. Unknown verb → MustNot.
///
/// Prefer [`classify_outcome`] when the handler needs the named refuse code
/// (`copy_not_in_profile`, `begin_not_in_profile`, …).
pub fn classify(profile: DialectProfile, protocol: ProtocolId, verb: &str) -> VerbClass {
    classify_outcome(profile, protocol, verb).class()
}

/// Classify + named refuse code for MustNot paths.
pub fn classify_outcome(
    profile: DialectProfile,
    protocol: ProtocolId,
    verb: &str,
) -> ClassifyOutcome {
    let v = normalize_verb(verb);
    match protocol {
        ProtocolId::PostgreSql => classify_pg(profile, &v),
        ProtocolId::Redis => classify_redis(profile, &v),
        ProtocolId::Cassandra => classify_cassandra(profile, &v),
        ProtocolId::Elasticsearch => classify_elasticsearch(profile, &v),
        ProtocolId::ClickHouse | ProtocolId::ClickHouseHttp => classify_clickhouse(profile, &v),
        ProtocolId::S3 => classify_s3(profile, &v),
        ProtocolId::WebDav => classify_webdav(profile, &v),
    }
}

/// Convenience: FirstBinary PostgreSQL classify (handlers often hard-code FB).
pub fn classify_pg_verb(profile: DialectProfile, verb: &str) -> ClassifyOutcome {
    classify_outcome(profile, ProtocolId::PostgreSql, verb)
}

/// Convenience: Redis classify.
pub fn classify_redis_verb(profile: DialectProfile, verb: &str) -> ClassifyOutcome {
    classify_outcome(profile, ProtocolId::Redis, verb)
}

fn before_complete(profile: DialectProfile) -> bool {
    matches!(
        profile,
        DialectProfile::FirstBinary | DialectProfile::HandlersComplete
    )
}

fn no_handler(profile: DialectProfile) -> bool {
    matches!(profile, DialectProfile::FirstBinary)
}

fn classify_pg(profile: DialectProfile, v: &str) -> ClassifyOutcome {
    // Always MustNot everywhere
    match v {
        "SERIALIZABLE"
        | "LISTEN"
        | "NOTIFY"
        | "PLPGSQL"
        | "TRIGGER"
        | "EXTENSION"
        | "FDW"
        | "REPLICATION"
        | "COPY_PROGRAM"
        | "WITH_HOLD"
        | "SCROLL"
        | "FETCH_BACKWARD" => {
            return ClassifyOutcome::MustNot(generic_must_not(ProtocolId::PostgreSql, v));
        }
        _ => {}
    }

    // Profile-gated txn / COPY / cursors
    match v {
        "BEGIN" | "COMMIT" | "ROLLBACK" | "START" => {
            return if before_complete(profile) {
                ClassifyOutcome::MustNot(CompatError::BeginNotInProfile)
            } else {
                ClassifyOutcome::Must
            };
        }
        "COPY" => {
            return if before_complete(profile) {
                ClassifyOutcome::MustNot(CompatError::CopyNotInProfile)
            } else {
                ClassifyOutcome::Must
            };
        }
        "DECLARE" | "FETCH" | "CLOSE" | "DECLARE_CURSOR" => {
            return if before_complete(profile) {
                ClassifyOutcome::MustNot(CompatError::CursorNotInProfile)
            } else {
                ClassifyOutcome::Must
            };
        }
        "EXPLAIN" => {
            return if matches!(profile, DialectProfile::CompleteProduct) {
                ClassifyOutcome::Must
            } else {
                // MAY in FB/HC — treat as Must so stock clients can probe; not a MUST NOT
                ClassifyOutcome::Must
            };
        }
        _ => {}
    }

    // First-binary / all-profile MUST core
    match v {
        "SELECT" | "INSERT" | "UPDATE" | "DELETE" | "CREATE" | "ALTER" | "DROP"
        | "PREPARE" | "EXECUTE" | "DEALLOCATE" | "PARSE" | "BIND" | "DESCRIBE" | "SYNC"
        | "SIMPLE_QUERY" | "EXTENDED_QUERY" | "SET" | "SHOW" | "DISCARD" => {
            ClassifyOutcome::Must
        }
        _ => ClassifyOutcome::MustNot(generic_must_not(ProtocolId::PostgreSql, v)),
    }
}

/// Redis resource group (spec FR-002) a capability-table command belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceGroup {
    ConnectionMeta,
    KeyspaceIntrospection,
    StringExpiry,
    Hash,
    List,
    Set,
    SortedSet,
    Stream,
    Bitmap,
    PubSub,
    Transaction,
}

/// One complete-product Redis capability-table row (`017`).
///
/// `name` is the upper-case command, possibly with a subcommand
/// (`"COMMAND INFO"`). `options` lists the accepted options; an unknown option
/// of a listed command is `MustNot` (handlers consult [`redis_option_supported`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityCommand {
    pub name: &'static str,
    pub group: ResourceGroup,
    pub profile: DialectProfile,
    pub options: &'static [&'static str],
}

const CP: DialectProfile = DialectProfile::CompleteProduct;

/// The complete-product Redis capability table
/// (`specs/017-redis-dialect-coverage/contracts/capability-table.md`).
pub const REDIS_CAPABILITY_TABLE: &[CapabilityCommand] = &[
    // ---- R1: Connection / server meta ----
    CapabilityCommand { name: "AUTH", group: ResourceGroup::ConnectionMeta, profile: CP, options: &[] },
    CapabilityCommand { name: "PING", group: ResourceGroup::ConnectionMeta, profile: CP, options: &[] },
    CapabilityCommand { name: "ECHO", group: ResourceGroup::ConnectionMeta, profile: CP, options: &[] },
    CapabilityCommand { name: "QUIT", group: ResourceGroup::ConnectionMeta, profile: CP, options: &[] },
    CapabilityCommand { name: "SELECT", group: ResourceGroup::ConnectionMeta, profile: CP, options: &[] },
    CapabilityCommand { name: "COMMAND", group: ResourceGroup::ConnectionMeta, profile: CP, options: &["INFO", "COUNT"] },
    CapabilityCommand { name: "COMMAND INFO", group: ResourceGroup::ConnectionMeta, profile: CP, options: &[] },
    CapabilityCommand { name: "COMMAND COUNT", group: ResourceGroup::ConnectionMeta, profile: CP, options: &[] },
    CapabilityCommand { name: "CLIENT", group: ResourceGroup::ConnectionMeta, profile: CP, options: &["ID", "INFO", "LIST", "KILL", "SETNAME", "GETNAME", "PAUSE"] },
    CapabilityCommand { name: "CLIENT ID", group: ResourceGroup::ConnectionMeta, profile: CP, options: &[] },
    CapabilityCommand { name: "CLIENT INFO", group: ResourceGroup::ConnectionMeta, profile: CP, options: &[] },
    CapabilityCommand { name: "CLIENT LIST", group: ResourceGroup::ConnectionMeta, profile: CP, options: &[] },
    CapabilityCommand { name: "CLIENT KILL", group: ResourceGroup::ConnectionMeta, profile: CP, options: &[] },
    CapabilityCommand { name: "CLIENT SETNAME", group: ResourceGroup::ConnectionMeta, profile: CP, options: &[] },
    CapabilityCommand { name: "CLIENT GETNAME", group: ResourceGroup::ConnectionMeta, profile: CP, options: &[] },
    CapabilityCommand { name: "CLIENT PAUSE", group: ResourceGroup::ConnectionMeta, profile: CP, options: &[] },
    CapabilityCommand { name: "CONFIG GET", group: ResourceGroup::ConnectionMeta, profile: CP, options: &[] },
    CapabilityCommand { name: "CONFIG SET", group: ResourceGroup::ConnectionMeta, profile: CP, options: &[] },
    CapabilityCommand { name: "TIME", group: ResourceGroup::ConnectionMeta, profile: CP, options: &[] },
    // ---- R2: Keyspace introspection ----
    CapabilityCommand { name: "INFO", group: ResourceGroup::KeyspaceIntrospection, profile: CP, options: &["SERVER", "CLIENTS", "MEMORY", "STATS", "KEYSPACE", "REPLICATION"] },
    CapabilityCommand { name: "DBSIZE", group: ResourceGroup::KeyspaceIntrospection, profile: CP, options: &[] },
    CapabilityCommand { name: "TYPE", group: ResourceGroup::KeyspaceIntrospection, profile: CP, options: &[] },
    CapabilityCommand { name: "TTL", group: ResourceGroup::KeyspaceIntrospection, profile: CP, options: &[] },
    CapabilityCommand { name: "PTTL", group: ResourceGroup::KeyspaceIntrospection, profile: CP, options: &[] },
    CapabilityCommand { name: "EXISTS", group: ResourceGroup::KeyspaceIntrospection, profile: CP, options: &[] },
    CapabilityCommand { name: "SCAN", group: ResourceGroup::KeyspaceIntrospection, profile: CP, options: &["MATCH", "COUNT"] },
    CapabilityCommand { name: "RANDOMKEY", group: ResourceGroup::KeyspaceIntrospection, profile: CP, options: &[] },
    CapabilityCommand { name: "OBJECT ENCODING", group: ResourceGroup::KeyspaceIntrospection, profile: CP, options: &[] },
    CapabilityCommand { name: "OBJECT IDLETIME", group: ResourceGroup::KeyspaceIntrospection, profile: CP, options: &[] },
    CapabilityCommand { name: "TOUCH", group: ResourceGroup::KeyspaceIntrospection, profile: CP, options: &[] },
    CapabilityCommand { name: "UNLINK", group: ResourceGroup::KeyspaceIntrospection, profile: CP, options: &[] },
    // ---- R3: Strings + expiry ----
    CapabilityCommand { name: "GET", group: ResourceGroup::StringExpiry, profile: CP, options: &[] },
    CapabilityCommand { name: "SET", group: ResourceGroup::StringExpiry, profile: CP, options: &["NX", "XX", "EX", "PX", "KEEPTTL"] },
    CapabilityCommand { name: "DEL", group: ResourceGroup::StringExpiry, profile: CP, options: &[] },
    CapabilityCommand { name: "MGET", group: ResourceGroup::StringExpiry, profile: CP, options: &[] },
    CapabilityCommand { name: "MSET", group: ResourceGroup::StringExpiry, profile: CP, options: &[] },
    CapabilityCommand { name: "MSETNX", group: ResourceGroup::StringExpiry, profile: CP, options: &[] },
    CapabilityCommand { name: "SETNX", group: ResourceGroup::StringExpiry, profile: CP, options: &[] },
    CapabilityCommand { name: "SETEX", group: ResourceGroup::StringExpiry, profile: CP, options: &[] },
    CapabilityCommand { name: "PSETEX", group: ResourceGroup::StringExpiry, profile: CP, options: &[] },
    CapabilityCommand { name: "GETSET", group: ResourceGroup::StringExpiry, profile: CP, options: &[] },
    CapabilityCommand { name: "STRLEN", group: ResourceGroup::StringExpiry, profile: CP, options: &[] },
    CapabilityCommand { name: "APPEND", group: ResourceGroup::StringExpiry, profile: CP, options: &[] },
    CapabilityCommand { name: "GETRANGE", group: ResourceGroup::StringExpiry, profile: CP, options: &[] },
    CapabilityCommand { name: "SETRANGE", group: ResourceGroup::StringExpiry, profile: CP, options: &[] },
    CapabilityCommand { name: "INCR", group: ResourceGroup::StringExpiry, profile: CP, options: &[] },
    CapabilityCommand { name: "DECR", group: ResourceGroup::StringExpiry, profile: CP, options: &[] },
    CapabilityCommand { name: "INCRBY", group: ResourceGroup::StringExpiry, profile: CP, options: &[] },
    CapabilityCommand { name: "DECRBY", group: ResourceGroup::StringExpiry, profile: CP, options: &[] },
    CapabilityCommand { name: "INCRBYFLOAT", group: ResourceGroup::StringExpiry, profile: CP, options: &[] },
    CapabilityCommand { name: "GETDEL", group: ResourceGroup::StringExpiry, profile: CP, options: &[] },
    CapabilityCommand { name: "GETEX", group: ResourceGroup::StringExpiry, profile: CP, options: &["EX", "PX", "EXAT", "PXAT", "PERSIST"] },
    CapabilityCommand { name: "EXPIRE", group: ResourceGroup::StringExpiry, profile: CP, options: &["NX", "XX", "GT", "LT"] },
    CapabilityCommand { name: "PEXPIRE", group: ResourceGroup::StringExpiry, profile: CP, options: &["NX", "XX", "GT", "LT"] },
    CapabilityCommand { name: "EXPIREAT", group: ResourceGroup::StringExpiry, profile: CP, options: &["NX", "XX", "GT", "LT"] },
    CapabilityCommand { name: "PEXPIREAT", group: ResourceGroup::StringExpiry, profile: CP, options: &["NX", "XX", "GT", "LT"] },
    CapabilityCommand { name: "PERSIST", group: ResourceGroup::StringExpiry, profile: CP, options: &[] },
    CapabilityCommand { name: "EXPIRETIME", group: ResourceGroup::StringExpiry, profile: CP, options: &[] },
    CapabilityCommand { name: "PEXPIRETIME", group: ResourceGroup::StringExpiry, profile: CP, options: &[] },
    // ---- R4: Hash (incl. hash-field expiry) ----
    CapabilityCommand { name: "HSET", group: ResourceGroup::Hash, profile: CP, options: &[] },
    CapabilityCommand { name: "HGET", group: ResourceGroup::Hash, profile: CP, options: &[] },
    CapabilityCommand { name: "HMSET", group: ResourceGroup::Hash, profile: CP, options: &[] },
    CapabilityCommand { name: "HMGET", group: ResourceGroup::Hash, profile: CP, options: &[] },
    CapabilityCommand { name: "HGETALL", group: ResourceGroup::Hash, profile: CP, options: &[] },
    CapabilityCommand { name: "HKEYS", group: ResourceGroup::Hash, profile: CP, options: &[] },
    CapabilityCommand { name: "HVALS", group: ResourceGroup::Hash, profile: CP, options: &[] },
    CapabilityCommand { name: "HLEN", group: ResourceGroup::Hash, profile: CP, options: &[] },
    CapabilityCommand { name: "HEXISTS", group: ResourceGroup::Hash, profile: CP, options: &[] },
    CapabilityCommand { name: "HDEL", group: ResourceGroup::Hash, profile: CP, options: &[] },
    CapabilityCommand { name: "HSETNX", group: ResourceGroup::Hash, profile: CP, options: &[] },
    CapabilityCommand { name: "HINCRBY", group: ResourceGroup::Hash, profile: CP, options: &[] },
    CapabilityCommand { name: "HINCRBYFLOAT", group: ResourceGroup::Hash, profile: CP, options: &[] },
    CapabilityCommand { name: "HSTRLEN", group: ResourceGroup::Hash, profile: CP, options: &[] },
    CapabilityCommand { name: "HRANDFIELD", group: ResourceGroup::Hash, profile: CP, options: &["WITHVALUES"] },
    CapabilityCommand { name: "HSCAN", group: ResourceGroup::Hash, profile: CP, options: &["MATCH", "COUNT", "NOVALUES"] },
    CapabilityCommand { name: "HEXPIRE", group: ResourceGroup::Hash, profile: CP, options: &["NX", "XX", "GT", "LT"] },
    CapabilityCommand { name: "HPEXPIRE", group: ResourceGroup::Hash, profile: CP, options: &["NX", "XX", "GT", "LT"] },
    CapabilityCommand { name: "HEXPIRETIME", group: ResourceGroup::Hash, profile: CP, options: &[] },
    CapabilityCommand { name: "HPEXPIRETIME", group: ResourceGroup::Hash, profile: CP, options: &[] },
    CapabilityCommand { name: "HEXPIREAT", group: ResourceGroup::Hash, profile: CP, options: &["NX", "XX", "GT", "LT"] },
    CapabilityCommand { name: "HPEXPIREAT", group: ResourceGroup::Hash, profile: CP, options: &["NX", "XX", "GT", "LT"] },
    CapabilityCommand { name: "HTTL", group: ResourceGroup::Hash, profile: CP, options: &[] },
    CapabilityCommand { name: "HPTTL", group: ResourceGroup::Hash, profile: CP, options: &[] },
    CapabilityCommand { name: "HGETEX", group: ResourceGroup::Hash, profile: CP, options: &["EX", "PX", "EXAT", "PXAT", "PERSIST"] },
    CapabilityCommand { name: "HGETDEL", group: ResourceGroup::Hash, profile: CP, options: &[] },
    // ---- R5: List ----
    CapabilityCommand { name: "LPUSH", group: ResourceGroup::List, profile: CP, options: &[] },
    CapabilityCommand { name: "RPUSH", group: ResourceGroup::List, profile: CP, options: &[] },
    CapabilityCommand { name: "LPOP", group: ResourceGroup::List, profile: CP, options: &[] },
    CapabilityCommand { name: "RPOP", group: ResourceGroup::List, profile: CP, options: &[] },
    CapabilityCommand { name: "LPUSHX", group: ResourceGroup::List, profile: CP, options: &[] },
    CapabilityCommand { name: "RPUSHX", group: ResourceGroup::List, profile: CP, options: &[] },
    CapabilityCommand { name: "LLEN", group: ResourceGroup::List, profile: CP, options: &[] },
    CapabilityCommand { name: "LRANGE", group: ResourceGroup::List, profile: CP, options: &[] },
    CapabilityCommand { name: "LINDEX", group: ResourceGroup::List, profile: CP, options: &[] },
    CapabilityCommand { name: "LSET", group: ResourceGroup::List, profile: CP, options: &[] },
    CapabilityCommand { name: "LREM", group: ResourceGroup::List, profile: CP, options: &[] },
    CapabilityCommand { name: "LTRIM", group: ResourceGroup::List, profile: CP, options: &[] },
    CapabilityCommand { name: "LINSERT", group: ResourceGroup::List, profile: CP, options: &["BEFORE", "AFTER"] },
    CapabilityCommand { name: "RPOPLPUSH", group: ResourceGroup::List, profile: CP, options: &[] },
    CapabilityCommand { name: "BLPOP", group: ResourceGroup::List, profile: CP, options: &[] },
    CapabilityCommand { name: "BRPOP", group: ResourceGroup::List, profile: CP, options: &[] },
    CapabilityCommand { name: "BRPOPLPUSH", group: ResourceGroup::List, profile: CP, options: &[] },
    CapabilityCommand { name: "LMOVE", group: ResourceGroup::List, profile: CP, options: &["LEFT", "RIGHT"] },
    CapabilityCommand { name: "BLMOVE", group: ResourceGroup::List, profile: CP, options: &["LEFT", "RIGHT"] },
    // ---- R6: Set ----
    CapabilityCommand { name: "SADD", group: ResourceGroup::Set, profile: CP, options: &[] },
    CapabilityCommand { name: "SREM", group: ResourceGroup::Set, profile: CP, options: &[] },
    CapabilityCommand { name: "SISMEMBER", group: ResourceGroup::Set, profile: CP, options: &[] },
    CapabilityCommand { name: "SMEMBERS", group: ResourceGroup::Set, profile: CP, options: &[] },
    CapabilityCommand { name: "SCARD", group: ResourceGroup::Set, profile: CP, options: &[] },
    CapabilityCommand { name: "SPOP", group: ResourceGroup::Set, profile: CP, options: &[] },
    CapabilityCommand { name: "SRANDMEMBER", group: ResourceGroup::Set, profile: CP, options: &[] },
    CapabilityCommand { name: "SSCAN", group: ResourceGroup::Set, profile: CP, options: &["MATCH", "COUNT"] },
    CapabilityCommand { name: "SUNION", group: ResourceGroup::Set, profile: CP, options: &[] },
    CapabilityCommand { name: "SUNIONSTORE", group: ResourceGroup::Set, profile: CP, options: &[] },
    CapabilityCommand { name: "SINTER", group: ResourceGroup::Set, profile: CP, options: &[] },
    CapabilityCommand { name: "SINTERSTORE", group: ResourceGroup::Set, profile: CP, options: &[] },
    CapabilityCommand { name: "SDIFF", group: ResourceGroup::Set, profile: CP, options: &[] },
    CapabilityCommand { name: "SDIFFSTORE", group: ResourceGroup::Set, profile: CP, options: &[] },
    // ---- R7: Sorted set ----
    CapabilityCommand { name: "ZADD", group: ResourceGroup::SortedSet, profile: CP, options: &["NX", "XX", "GT", "LT", "CH", "INCR"] },
    CapabilityCommand { name: "ZREM", group: ResourceGroup::SortedSet, profile: CP, options: &[] },
    CapabilityCommand { name: "ZCARD", group: ResourceGroup::SortedSet, profile: CP, options: &[] },
    CapabilityCommand { name: "ZSCORE", group: ResourceGroup::SortedSet, profile: CP, options: &[] },
    CapabilityCommand { name: "ZMSCORE", group: ResourceGroup::SortedSet, profile: CP, options: &[] },
    CapabilityCommand { name: "ZCOUNT", group: ResourceGroup::SortedSet, profile: CP, options: &[] },
    CapabilityCommand { name: "ZRANK", group: ResourceGroup::SortedSet, profile: CP, options: &["WITHSCORE"] },
    CapabilityCommand { name: "ZREVRANK", group: ResourceGroup::SortedSet, profile: CP, options: &["WITHSCORE"] },
    CapabilityCommand { name: "ZRANGE", group: ResourceGroup::SortedSet, profile: CP, options: &["BYSCORE", "BYLEX", "REV", "LIMIT", "WITHSCORES"] },
    CapabilityCommand { name: "ZREVRANGE", group: ResourceGroup::SortedSet, profile: CP, options: &["WITHSCORES"] },
    CapabilityCommand { name: "ZRANGEBYSCORE", group: ResourceGroup::SortedSet, profile: CP, options: &["WITHSCORES", "LIMIT"] },
    CapabilityCommand { name: "ZREVRANGEBYSCORE", group: ResourceGroup::SortedSet, profile: CP, options: &["WITHSCORES", "LIMIT"] },
    CapabilityCommand { name: "ZPOPMIN", group: ResourceGroup::SortedSet, profile: CP, options: &[] },
    CapabilityCommand { name: "ZPOPMAX", group: ResourceGroup::SortedSet, profile: CP, options: &[] },
    CapabilityCommand { name: "BZPOPMIN", group: ResourceGroup::SortedSet, profile: CP, options: &[] },
    CapabilityCommand { name: "BZPOPMAX", group: ResourceGroup::SortedSet, profile: CP, options: &[] },
    CapabilityCommand { name: "ZUNIONSTORE", group: ResourceGroup::SortedSet, profile: CP, options: &["WEIGHTS", "AGGREGATE"] },
    CapabilityCommand { name: "ZINTERSTORE", group: ResourceGroup::SortedSet, profile: CP, options: &["WEIGHTS", "AGGREGATE"] },
    CapabilityCommand { name: "ZDIFFSTORE", group: ResourceGroup::SortedSet, profile: CP, options: &[] },
    CapabilityCommand { name: "ZUNION", group: ResourceGroup::SortedSet, profile: CP, options: &["WEIGHTS", "AGGREGATE", "WITHSCORES"] },
    CapabilityCommand { name: "ZINTER", group: ResourceGroup::SortedSet, profile: CP, options: &["WEIGHTS", "AGGREGATE", "WITHSCORES"] },
    CapabilityCommand { name: "ZDIFF", group: ResourceGroup::SortedSet, profile: CP, options: &["WITHSCORES"] },
    CapabilityCommand { name: "ZSCAN", group: ResourceGroup::SortedSet, profile: CP, options: &["MATCH", "COUNT"] },
    CapabilityCommand { name: "ZINCRBY", group: ResourceGroup::SortedSet, profile: CP, options: &[] },
    CapabilityCommand { name: "ZREMRANGEBYSCORE", group: ResourceGroup::SortedSet, profile: CP, options: &[] },
    CapabilityCommand { name: "ZREMRANGEBYRANK", group: ResourceGroup::SortedSet, profile: CP, options: &[] },
    CapabilityCommand { name: "ZRANGESTORE", group: ResourceGroup::SortedSet, profile: CP, options: &["BYSCORE", "BYLEX", "REV", "LIMIT"] },
    // ---- R8: Streams + consumer groups ----
    CapabilityCommand { name: "XADD", group: ResourceGroup::Stream, profile: CP, options: &["NOMKSTREAM", "MAXLEN", "MINID", "LIMIT"] },
    CapabilityCommand { name: "XDEL", group: ResourceGroup::Stream, profile: CP, options: &[] },
    CapabilityCommand { name: "XRANGE", group: ResourceGroup::Stream, profile: CP, options: &["COUNT"] },
    CapabilityCommand { name: "XREVRANGE", group: ResourceGroup::Stream, profile: CP, options: &["COUNT"] },
    CapabilityCommand { name: "XLEN", group: ResourceGroup::Stream, profile: CP, options: &[] },
    CapabilityCommand { name: "XTRIM", group: ResourceGroup::Stream, profile: CP, options: &["MAXLEN", "MINID", "LIMIT"] },
    CapabilityCommand { name: "XREAD", group: ResourceGroup::Stream, profile: CP, options: &["COUNT", "BLOCK", "STREAMS"] },
    CapabilityCommand { name: "XREADGROUP", group: ResourceGroup::Stream, profile: CP, options: &["GROUP", "COUNT", "BLOCK", "NOACK", "STREAMS"] },
    CapabilityCommand { name: "XGROUP", group: ResourceGroup::Stream, profile: CP, options: &["CREATE", "DESTROY", "SETID", "DELCONSUMER", "CREATECONSUMER", "HELP"] },
    CapabilityCommand { name: "XGROUP CREATE", group: ResourceGroup::Stream, profile: CP, options: &["MKSTREAM"] },
    CapabilityCommand { name: "XGROUP DESTROY", group: ResourceGroup::Stream, profile: CP, options: &[] },
    CapabilityCommand { name: "XGROUP SETID", group: ResourceGroup::Stream, profile: CP, options: &[] },
    CapabilityCommand { name: "XGROUP DELCONSUMER", group: ResourceGroup::Stream, profile: CP, options: &[] },
    CapabilityCommand { name: "XGROUP CREATECONSUMER", group: ResourceGroup::Stream, profile: CP, options: &[] },
    CapabilityCommand { name: "XGROUP HELP", group: ResourceGroup::Stream, profile: CP, options: &[] },
    CapabilityCommand { name: "XACK", group: ResourceGroup::Stream, profile: CP, options: &[] },
    CapabilityCommand { name: "XPENDING", group: ResourceGroup::Stream, profile: CP, options: &["IDLE"] },
    CapabilityCommand { name: "XCLAIM", group: ResourceGroup::Stream, profile: CP, options: &["IDLE", "TIME", "RETRYCOUNT", "FORCE", "JUSTID", "LASTID"] },
    CapabilityCommand { name: "XINFO", group: ResourceGroup::Stream, profile: CP, options: &["STREAM", "GROUPS", "CONSUMERS", "HELP"] },
    CapabilityCommand { name: "XINFO STREAM", group: ResourceGroup::Stream, profile: CP, options: &["FULL"] },
    CapabilityCommand { name: "XINFO GROUPS", group: ResourceGroup::Stream, profile: CP, options: &[] },
    CapabilityCommand { name: "XINFO CONSUMERS", group: ResourceGroup::Stream, profile: CP, options: &[] },
    // ---- R9: Bitmaps ----
    CapabilityCommand { name: "SETBIT", group: ResourceGroup::Bitmap, profile: CP, options: &[] },
    CapabilityCommand { name: "GETBIT", group: ResourceGroup::Bitmap, profile: CP, options: &[] },
    CapabilityCommand { name: "BITCOUNT", group: ResourceGroup::Bitmap, profile: CP, options: &["BYTE", "BIT"] },
    CapabilityCommand { name: "BITPOS", group: ResourceGroup::Bitmap, profile: CP, options: &["BYTE", "BIT"] },
    CapabilityCommand { name: "BITOP", group: ResourceGroup::Bitmap, profile: CP, options: &["AND", "OR", "XOR", "NOT"] },
    CapabilityCommand { name: "BITFIELD", group: ResourceGroup::Bitmap, profile: CP, options: &["GET", "SET", "INCRBY", "OVERFLOW"] },
    CapabilityCommand { name: "BITFIELD_RO", group: ResourceGroup::Bitmap, profile: CP, options: &["GET"] },
    // ---- R9: HyperLogLog ----
    CapabilityCommand { name: "PFADD", group: ResourceGroup::Bitmap, profile: CP, options: &[] },
    CapabilityCommand { name: "PFCOUNT", group: ResourceGroup::Bitmap, profile: CP, options: &[] },
    CapabilityCommand { name: "PFMERGE", group: ResourceGroup::Bitmap, profile: CP, options: &[] },
    // ---- Pub/Sub (connection-scoped) ----
    CapabilityCommand { name: "SUBSCRIBE", group: ResourceGroup::PubSub, profile: CP, options: &[] },
    CapabilityCommand { name: "UNSUBSCRIBE", group: ResourceGroup::PubSub, profile: CP, options: &[] },
    CapabilityCommand { name: "PSUBSCRIBE", group: ResourceGroup::PubSub, profile: CP, options: &[] },
    CapabilityCommand { name: "PUNSUBSCRIBE", group: ResourceGroup::PubSub, profile: CP, options: &[] },
    CapabilityCommand { name: "PUBLISH", group: ResourceGroup::PubSub, profile: CP, options: &[] },
    CapabilityCommand { name: "PUBSUB", group: ResourceGroup::PubSub, profile: CP, options: &["CHANNELS", "NUMSUB", "NUMPAT"] },
    CapabilityCommand { name: "SPUBLISH", group: ResourceGroup::PubSub, profile: CP, options: &[] },
    CapabilityCommand { name: "SSUBSCRIBE", group: ResourceGroup::PubSub, profile: CP, options: &[] },
    CapabilityCommand { name: "SUNSUBSCRIBE", group: ResourceGroup::PubSub, profile: CP, options: &[] },
    // ---- Transactions (connection-scoped) ----
    CapabilityCommand { name: "MULTI", group: ResourceGroup::Transaction, profile: CP, options: &[] },
    CapabilityCommand { name: "EXEC", group: ResourceGroup::Transaction, profile: CP, options: &[] },
    CapabilityCommand { name: "DISCARD", group: ResourceGroup::Transaction, profile: CP, options: &[] },
    CapabilityCommand { name: "WATCH", group: ResourceGroup::Transaction, profile: CP, options: &[] },
    CapabilityCommand { name: "UNWATCH", group: ResourceGroup::Transaction, profile: CP, options: &[] },
];

/// The always-`MustNot` closed not-supported list (canonical
/// `specs/017-redis-dialect-coverage/contracts/not-supported.md`). Enforced on
/// every profile, including complete product.
pub const REDIS_NOT_SUPPORTED: &[&str] = &[
    // 1. Geospatial
    "GEOADD", "GEODIST", "GEOPOS", "GEOHASH", "GEORADIUS", "GEORADIUS_RO",
    "GEORADIUSBYMEMBER", "GEORADIUSBYMEMBER_RO", "GEOSEARCH", "GEOSEARCHSTORE",
    // 2. Stream auto-claim
    "XAUTOCLAIM",
    // 3. Lua / scripting
    "EVAL", "EVALSHA", "SCRIPT",
    // 4. Functions
    "FUNCTION", "FCALL", "FCALL_RO",
    // 5. Modules
    "MODULE",
    // 6. Cluster
    "CLUSTER", "CLUSTER_SLOTS",
    // 7. Sentinel
    "SENTINEL",
    // 8. ACL management (AUTH itself stays MUST)
    "ACL",
    // 9. Historical / deprecated
    "DEBUG", "SHUTDOWN", "SLAVEOF", "REPLICAOF", "SYNC", "PSYNC", "MIGRATE",
    "RESTORE", "DUMP", "RENAME", "RENAMENX",
    // 10. RESP3-only
    "HELLO", "CLIENT_SETINFO", "RESP3",
    // 11. Database selection / swap (SELECT n>0 handled at option level)
    "SWAPDB",
];

fn base_verb(v: &str) -> &str {
    v.split([' ', ':']).next().unwrap_or(v)
}

/// Look up a command in the complete-product capability table. Matches an exact
/// name (`"COMMAND INFO"`) or the base command token (`"COMMAND"`).
pub fn redis_capability(verb: &str) -> Option<&'static CapabilityCommand> {
    let v = normalize_verb(verb);
    REDIS_CAPABILITY_TABLE
        .iter()
        .find(|c| c.name == v.as_str())
        .or_else(|| {
            let base = base_verb(&v);
            REDIS_CAPABILITY_TABLE.iter().find(|c| c.name == base)
        })
}

/// True when `verb` is on the closed not-supported list.
pub fn redis_is_not_supported(verb: &str) -> bool {
    let v = normalize_verb(verb);
    REDIS_NOT_SUPPORTED.contains(&base_verb(&v))
}

/// Check an option of a listed command. An unknown option → `false` (MustNot).
pub fn redis_option_supported(verb: &str, option: &str) -> bool {
    let Some(cmd) = redis_capability(verb) else {
        return false;
    };
    let o = normalize_verb(option);
    cmd.options.is_empty() || cmd.options.contains(&o.as_str())
}

fn classify_redis(profile: DialectProfile, v: &str) -> ClassifyOutcome {
    // Closed not-supported list → MustNot on every profile (research R11/R13).
    if redis_is_not_supported(v) {
        return ClassifyOutcome::MustNot(generic_must_not(ProtocolId::Redis, v));
    }
    // First-binary / handlers-complete: the `016` K/V string list is frozen.
    if before_complete(profile) {
        return match v {
            "AUTH" | "PING" | "GET" | "SET" | "DEL" | "EXISTS" | "SCAN" | "SELECT" | "TTL"
            | "PTTL" | "EXPIRE" => ClassifyOutcome::Must,
            _ => ClassifyOutcome::MustNot(generic_must_not(ProtocolId::Redis, v)),
        };
    }
    // Complete product: the capability table is the MUST set (research R1).
    if redis_capability(v).is_some() {
        ClassifyOutcome::Must
    } else {
        ClassifyOutcome::MustNot(generic_must_not(ProtocolId::Redis, v))
    }
}

fn classify_cassandra(profile: DialectProfile, v: &str) -> ClassifyOutcome {
    if no_handler(profile) {
        return ClassifyOutcome::MustNot(generic_must_not(ProtocolId::Cassandra, v));
    }
    match v {
        "QUERY" | "EXECUTE" | "PREPARE" | "BATCH" | "STARTUP" | "OPTIONS" | "REGISTER"
        | "SELECT" | "INSERT" | "UPDATE" | "DELETE" | "CREATE" | "ALTER" | "DROP" | "USE" => {
            ClassifyOutcome::Must
        }
        "LWT" | "CREATE_MATERIALIZED_VIEW" | "CDC" => {
            ClassifyOutcome::MustNot(generic_must_not(ProtocolId::Cassandra, v))
        }
        _ => ClassifyOutcome::MustNot(generic_must_not(ProtocolId::Cassandra, v)),
    }
}

fn classify_elasticsearch(profile: DialectProfile, v: &str) -> ClassifyOutcome {
    if no_handler(profile) {
        return ClassifyOutcome::MustNot(generic_must_not(ProtocolId::Elasticsearch, v));
    }
    match v {
        "INDEX" | "GET" | "DELETE" | "CREATE_INDEX" | "CAT" | "LIST" | "SEARCH"
        | "SEARCH_QUERY_STRING" | "SEARCH_MATCH" | "SEARCH_TERM" | "SEARCH_RANGE"
        | "SEARCH_BOOL" => ClassifyOutcome::Must,
        "AGG" | "AGG_TERMS" | "AGG_MIN" | "AGG_MAX" | "AGG_SUM" | "AGG_AVG"
        | "AGG_HISTOGRAM" | "AGG_VALUE_COUNT" | "AGGREGATIONS" => {
            if matches!(profile, DialectProfile::CompleteProduct) {
                ClassifyOutcome::Must
            } else {
                ClassifyOutcome::MustNot(CompatError::AggNotInProfile)
            }
        }
        "ILM" | "INGEST" | "ML" | "CCR" | "SCRIPT" | "SIGNIFICANT_TERMS" | "COMPOSITE"
        | "DATE_HISTOGRAM" => {
            ClassifyOutcome::MustNot(generic_must_not(ProtocolId::Elasticsearch, v))
        }
        _ => ClassifyOutcome::MustNot(generic_must_not(ProtocolId::Elasticsearch, v)),
    }
}

fn classify_clickhouse(profile: DialectProfile, v: &str) -> ClassifyOutcome {
    if no_handler(profile) {
        return ClassifyOutcome::MustNot(generic_must_not(ProtocolId::ClickHouse, v));
    }
    match v {
        "CREATE" | "INSERT" | "SELECT" | "SELECT_WHERE" => ClassifyOutcome::Must,
        "GROUP_BY" | "SELECT_GROUP_BY" => {
            if matches!(profile, DialectProfile::CompleteProduct) {
                ClassifyOutcome::Must
            } else {
                ClassifyOutcome::MustNot(generic_must_not(ProtocolId::ClickHouse, v))
            }
        }
        "DICTIONARY" | "MATERIALIZED_VIEW" | "REPLICATION" => {
            ClassifyOutcome::MustNot(generic_must_not(ProtocolId::ClickHouse, v))
        }
        _ => ClassifyOutcome::MustNot(generic_must_not(ProtocolId::ClickHouse, v)),
    }
}

fn classify_s3(profile: DialectProfile, v: &str) -> ClassifyOutcome {
    if no_handler(profile) {
        return ClassifyOutcome::MustNot(generic_must_not(ProtocolId::S3, v));
    }
    match v {
        "LISTBUCKETS"
        | "HEADBUCKET"
        | "CREATEBUCKET"
        | "DELETEBUCKET"
        | "GETBUCKETLOCATION"
        | "PUTOBJECT"
        | "GETOBJECT"
        | "HEADOBJECT"
        | "DELETEOBJECT"
        | "DELETEOBJECTS"
        | "COPYOBJECT"
        | "LISTOBJECTSV2"
        | "LISTOBJECTS"
        | "CREATEMULTIPARTUPLOAD"
        | "UPLOADPART"
        | "COMPLETEMULTIPARTUPLOAD"
        | "ABORTMULTIPARTUPLOAD"
        | "LISTPARTS"
        | "LISTMULTIPARTUPLOADS"
        | "PRESIGNED" => ClassifyOutcome::Must,
        "VERSIONING"
        | "OBJECTLOCK"
        | "REPLICATION"
        | "SELECT"
        | "ACL"
        | "LIFECYCLE"
        | "CORS"
        | "TAGGING"
        | "SSEKMS"
        | "SIGV2"
        | "UPLOADPARTCOPY"
        | "GETOBJECTATTRIBUTES" => ClassifyOutcome::MustNot(generic_must_not(ProtocolId::S3, v)),
        _ => ClassifyOutcome::MustNot(generic_must_not(ProtocolId::S3, v)),
    }
}

fn classify_webdav(profile: DialectProfile, v: &str) -> ClassifyOutcome {
    if no_handler(profile) {
        return ClassifyOutcome::MustNot(generic_must_not(ProtocolId::WebDav, v));
    }
    match v {
        "PROPFIND" | "GET" | "PUT" | "DELETE" | "MKCOL" | "MOVE" | "COPY" => {
            ClassifyOutcome::Must
        }
        "LOCK" | "UNLOCK" | "CALDAV" | "CARDDAV" => {
            ClassifyOutcome::MustNot(generic_must_not(ProtocolId::WebDav, v))
        }
        _ => ClassifyOutcome::MustNot(generic_must_not(ProtocolId::WebDav, v)),
    }
}

/// Cursor rules (contracts/postgresql-cursors.md) — WITH HOLD / SCROLL always MustNot.
pub fn classify_pg_cursor(
    profile: DialectProfile,
    verb: &str,
    with_hold: bool,
    scroll: bool,
    backward: bool,
) -> ClassifyOutcome {
    if with_hold || scroll || backward {
        return ClassifyOutcome::MustNot(generic_must_not(
            ProtocolId::PostgreSql,
            if with_hold {
                "WITH_HOLD"
            } else if scroll {
                "SCROLL"
            } else {
                "FETCH_BACKWARD"
            },
        ));
    }
    classify_pg(profile, &normalize_verb(verb))
}

/// COPY format gate (contracts/copy.md). PROGRAM/FREEZE/server-path always MustNot.
pub fn classify_pg_copy(
    profile: DialectProfile,
    format: Option<&str>,
    program: bool,
    freeze: bool,
    server_path: bool,
) -> ClassifyOutcome {
    if program || freeze || server_path {
        return ClassifyOutcome::MustNot(generic_must_not(ProtocolId::PostgreSql, "COPY_PROGRAM"));
    }
    if let Some(fmt) = format {
        let f = fmt.to_ascii_lowercase();
        if !matches!(f.as_str(), "text" | "csv" | "binary")
            && matches!(profile, DialectProfile::CompleteProduct)
        {
            return ClassifyOutcome::MustNot(generic_must_not(ProtocolId::PostgreSql, "COPY"));
        }
    }
    classify_pg(profile, "COPY")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fb_pg_must_dml() {
        let p = DialectProfile::FirstBinary;
        for v in ["SELECT", "INSERT", "UPDATE", "DELETE", "CREATE", "DROP"] {
            assert_eq!(classify(p, ProtocolId::PostgreSql, v), VerbClass::Must, "{v}");
        }
    }

    #[test]
    fn fb_pg_must_not_begin_copy_declare() {
        let p = DialectProfile::FirstBinary;
        assert_eq!(
            classify_outcome(p, ProtocolId::PostgreSql, "BEGIN"),
            ClassifyOutcome::MustNot(CompatError::BeginNotInProfile)
        );
        assert_eq!(
            classify_outcome(p, ProtocolId::PostgreSql, "COPY"),
            ClassifyOutcome::MustNot(CompatError::CopyNotInProfile)
        );
        assert_eq!(
            classify_outcome(p, ProtocolId::PostgreSql, "DECLARE"),
            ClassifyOutcome::MustNot(CompatError::CursorNotInProfile)
        );
    }

    #[test]
    fn fb_redis_must_and_cluster_eval() {
        let p = DialectProfile::FirstBinary;
        for v in [
            "AUTH", "PING", "GET", "SET", "DEL", "EXISTS", "SCAN", "SELECT", "TTL", "PTTL",
            "EXPIRE",
        ] {
            assert_eq!(classify(p, ProtocolId::Redis, v), VerbClass::Must, "{v}");
        }
        for v in [
            "COMMAND", "INFO", "CONFIG", "TYPE", "DBSIZE", "ECHO", "QUIT", "PEXPIRE", "CLUSTER",
            "EVAL",
        ] {
            assert_eq!(
                classify(p, ProtocolId::Redis, v),
                VerbClass::MustNot,
                "{v}"
            );
        }
    }

    #[test]
    fn unknown_is_must_not() {
        assert_eq!(
            classify(
                DialectProfile::FirstBinary,
                ProtocolId::PostgreSql,
                "NO_SUCH_VERB"
            ),
            VerbClass::MustNot
        );
    }

    #[test]
    fn cp_begin_and_copy_must() {
        let p = DialectProfile::CompleteProduct;
        assert_eq!(classify(p, ProtocolId::PostgreSql, "BEGIN"), VerbClass::Must);
        assert_eq!(classify(p, ProtocolId::PostgreSql, "COPY"), VerbClass::Must);
        assert_eq!(
            classify(p, ProtocolId::PostgreSql, "DECLARE"),
            VerbClass::Must
        );
    }

    #[test]
    fn es_agg_gated() {
        assert_eq!(
            classify_outcome(
                DialectProfile::HandlersComplete,
                ProtocolId::Elasticsearch,
                "AGG_TERMS"
            ),
            ClassifyOutcome::MustNot(CompatError::AggNotInProfile)
        );
        assert_eq!(
            classify(
                DialectProfile::CompleteProduct,
                ProtocolId::Elasticsearch,
                "AGG_TERMS"
            ),
            VerbClass::Must
        );
    }

    #[test]
    fn webdav_head_options_must_not() {
        let p = DialectProfile::HandlersComplete;
        for verb in ["HEAD", "OPTIONS"] {
            assert!(
                matches!(
                    classify_outcome(p, ProtocolId::WebDav, verb),
                    ClassifyOutcome::MustNot(_)
                ),
                "{verb}"
            );
        }
        for verb in ["GET", "PROPFIND", "PUT", "DELETE", "MKCOL", "MOVE", "COPY"] {
            assert_eq!(
                classify_outcome(p, ProtocolId::WebDav, verb),
                ClassifyOutcome::Must
            );
        }
    }

    #[test]
    fn fb_has_no_es_handler_verbs() {
        assert_eq!(
            classify(
                DialectProfile::FirstBinary,
                ProtocolId::Elasticsearch,
                "SEARCH"
            ),
            VerbClass::MustNot
        );
    }

    #[test]
    fn hc_s3_supported_must_verbs() {
        let p = DialectProfile::HandlersComplete;
        for v in [
            "HEADBUCKET",
            "DELETEBUCKET",
            "DELETEOBJECTS",
            "COPYOBJECT",
            "LISTOBJECTS",
            "LISTPARTS",
            "ABORTMULTIPARTUPLOAD",
            "LISTMULTIPARTUPLOADS",
            "GETBUCKETLOCATION",
            "PRESIGNED",
        ] {
            assert_eq!(
                classify_outcome(p, ProtocolId::S3, v),
                ClassifyOutcome::Must,
                "{v}"
            );
        }
    }

    #[test]
    fn fb_s3_handler_absent_must_not() {
        let p = DialectProfile::FirstBinary;
        assert!(matches!(
            classify_outcome(p, ProtocolId::S3, "PUTOBJECT"),
            ClassifyOutcome::MustNot(_)
        ));
    }

    #[test]
    fn hc_s3_unsupported_by_design_must_not() {
        let p = DialectProfile::HandlersComplete;
        for v in [
            "VERSIONING",
            "OBJECTLOCK",
            "REPLICATION",
            "SELECT",
            "ACL",
            "LIFECYCLE",
            "CORS",
            "TAGGING",
            "SSEKMS",
            "SIGV2",
            "UPLOADPARTCOPY",
            "GETOBJECTATTRIBUTES",
        ] {
            assert!(
                matches!(
                    classify_outcome(p, ProtocolId::S3, v),
                    ClassifyOutcome::MustNot(_)
                ),
                "{v}"
            );
        }
    }
}

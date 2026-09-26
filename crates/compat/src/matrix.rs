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

fn classify_redis(profile: DialectProfile, v: &str) -> ClassifyOutcome {
    let _ = profile; // Redis MUST list identical across FB/HC/CP for the core set
    // Matrix FB MUST: AUTH…TTL on KV + TTL-family EXPIRE/PTTL (handler-redis).
    // COMMAND/INFO/CONFIG/TYPE/DBSIZE/ECHO/QUIT/PEXPIRE are outside MUST → MustNot.
    match v {
        "AUTH" | "PING" | "GET" | "SET" | "DEL" | "EXISTS" | "SCAN" | "SELECT" | "TTL"
        | "PTTL" | "EXPIRE" => ClassifyOutcome::Must,
        "COMMAND" | "INFO" | "CONFIG" | "TYPE" | "DBSIZE" | "ECHO" | "QUIT" | "PEXPIRE"
        | "CLUSTER" | "CLUSTER_SLOTS" | "EVAL" | "EVALSHA" | "SCRIPT" | "XADD" | "XREAD"
        | "XGROUP" | "MODULE" | "RESP3" | "HELLO" => {
            ClassifyOutcome::MustNot(generic_must_not(ProtocolId::Redis, v))
        }
        _ => ClassifyOutcome::MustNot(generic_must_not(ProtocolId::Redis, v)),
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
        "LISTBUCKETS" | "CREATEBUCKET" | "PUTOBJECT" | "GETOBJECT" | "DELETEOBJECT"
        | "CREATEMULTIPARTUPLOAD" | "UPLOADPART" | "COMPLETEMULTIPARTUPLOAD"
        | "LISTOBJECTSV2" | "HEADOBJECT" => ClassifyOutcome::Must,
        "VERSIONING" | "OBJECTLOCK" | "REPLICATION" | "SELECT" | "ACL" => {
            ClassifyOutcome::MustNot(generic_must_not(ProtocolId::S3, v))
        }
        _ => ClassifyOutcome::MustNot(generic_must_not(ProtocolId::S3, v)),
    }
}

fn classify_webdav(profile: DialectProfile, v: &str) -> ClassifyOutcome {
    if no_handler(profile) {
        return ClassifyOutcome::MustNot(generic_must_not(ProtocolId::WebDav, v));
    }
    match v {
        "PROPFIND" | "GET" | "PUT" | "DELETE" | "MKCOL" | "MOVE" | "COPY" | "HEAD"
        | "OPTIONS" => ClassifyOutcome::Must,
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
}

//! Compatibility ceiling — dialect profiles, MUST/MUST NOT matrix, isolation,
//! size/connection/admission limits, and product-version window (feature 015).
//!
//! # Handler integration (owned by `002`)
//!
//! Call **before** lowering to `LogicalRequest`:
//!
//! ```ignore
//! use spacestorage_compat::{
//!     classify_outcome, DialectProfile, ProtocolId, ClassifyOutcome,
//! };
//!
//! let profile = DialectProfile::FirstBinary; // or active_profile() / release map
//! match classify_outcome(profile, ProtocolId::PostgreSql, "COPY") {
//!     ClassifyOutcome::Must => { /* lower to IR */ }
//!     ClassifyOutcome::MustNot(err) => {
//!         // render via 002 ErrorRenderer (PG 0A000 / Redis -ERR); never empty success
//!         return render_not_supported(err);
//!     }
//! }
//! ```
//!
//! Redis: `classify_outcome(profile, ProtocolId::Redis, "GET")`.
//! Size / connections: [`Limits::check_value`], [`SessionCounters::try_accept_entrypoint`].
//! Concurrent queries: [`SessionCounters::try_admit_query`] (never hangs; spill ≠ slot).
//!
//! FirstBinary has **no** Elasticsearch/Cassandra/ClickHouse/S3/WebDAV handlers —
//! entrypoints naming those fail startup `unknown_handler` (016 / release-profile).
//!
//! This crate does **not** brand the seven-protocol matrix as “v1”; first binary
//! is a **subset** via [`DialectProfile::FirstBinary`]. Unmodified applications
//! that need MUST NOT verbs are out of scope; stock clients on MUST verbs are in scope.

pub mod error;
pub mod isolation;
pub mod limits;
pub mod matrix;
pub mod metrics;
pub mod profile;
pub mod upgrade;
pub mod version;
pub mod wire;

pub use error::CompatError;
pub use isolation::{
    check_snapshot_capable, check_snapshot_for_types, map_sql_isolation, IsolationLevel,
    IsolationRefuse, ALLOWED_ISOLATION_SET,
};
pub use limits::{
    check_size, AdmissionGuard, AdmissionPolicy, ByteCount, EffectiveLimits, LimitKind,
    LimitProvenance, Limits, SessionCounters, SpillMode, DEFAULT_MAX_CONNECTIONS_PER_ENTRYPOINT,
    DEFAULT_MAX_CONNECTIONS_PER_PRINCIPAL, DEFAULT_MAX_CONCURRENT_PER_NAMESPACE,
    DEFAULT_MAX_CONCURRENT_PER_NODE, DEFAULT_MAX_KEY, DEFAULT_MAX_QUERY_MEMORY,
    DEFAULT_MAX_QUERY_TEXT, DEFAULT_MAX_RESULT, DEFAULT_MAX_VALUE,
};
pub use matrix::{
    classify, classify_outcome, classify_pg_copy, classify_pg_cursor, classify_pg_verb,
    classify_redis_verb, ClassifyOutcome, VerbClass,
};
pub use metrics::{must_not_count, must_not_snapshot, record_must_not};
pub use profile::{active_profile, profile_from_release, DialectProfile};
pub use upgrade::{evaluate_peer_join, refuse_format_too_new, refuse_type_too_new, UpgradeJoin};
pub use version::{peers_ok, ProductVersion};
pub use wire::{
    ProtocolId, WireVersion, CASSANDRA_V4, CASSANDRA_V5, CLICKHOUSE_HTTP, CLICKHOUSE_NATIVE,
    ELASTICSEARCH_HTTP11, PG_WIRE_3_0, REDIS_RESP2, RESP3_IN_PROFILE, S3_SIGV4, WEBDAV_RFC4918,
};

/// Re-export wire_versions lookup.
pub use wire::wire_versions;

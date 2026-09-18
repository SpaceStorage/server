# Data Model: Compatibility Ceiling, Limits, Isolation, Version Window

**Feature**: `015-compatibility-and-limits` | **Date**: 2026-09-18 | **Source**: [spec.md](spec.md) Key Entities, [research.md](research.md)

Types in Rust-ish notation. Crate owner is `crates/compat` unless noted.

## 1. DialectProfile

```text
enum DialectProfile { FirstBinary, HandlersComplete, CompleteProduct }
```

Compiled from `release-profile` (`016`). FirstBinary ⊂ HandlersComplete ⊂ CompleteProduct. MUST NOT verbs of a wider profile remain MUST NOT on a narrower one. Adding a verb to MUST or MUST NOT is a spec change of this feature.

## 2. ProtocolId / WireVersion

```text
enum ProtocolId {
    PostgreSql, Cassandra, Redis, Elasticsearch,
    ClickHouse, ClickHouseHttp, S3, WebDav,
}

struct WireVersion {
    protocol: ProtocolId,
    label: &'static str,          // "3.0", "v4", "RESP2", …
}
```

Constants: PostgreSQL frontend/backend 3.0; Cassandra native v4 and v5; Redis RESP2; Elasticsearch HTTP/1.1; ClickHouse native Hello/query/data; ClickHouse HTTP query/insert; S3 SigV4 REST; WebDAV RFC 4918 subset. RESP3 is not in any current profile.

## 3. VerbClass / CompatibilityMatrix

```text
enum VerbClass { Must, MustNot }

fn classify(profile: DialectProfile, protocol: ProtocolId, verb: &Verb) -> VerbClass
```

`Verb` is a protocol-specific enum (SQL statement kind, Redis command, ES path+method, S3 op, WebDAV method, CQL opcode). Unknown verb → `MustNot`. Handlers MUST call `classify` before `LogicalRequest`. `MustNot` → `002` `ErrorRenderer`; no IR, no storage.

PostgreSQL cursor subset: [postgresql-cursors.md](contracts/postgresql-cursors.md). Elasticsearch subset: [elasticsearch-search.md](contracts/elasticsearch-search.md). Full tables: [matrix.md](contracts/matrix.md).

## 4. IsolationSet

```text
enum IsolationLevel { ReadCommitted, Snapshot }

fn map_sql_isolation(text: &str) -> Result<IsolationLevel, IsolationRefuse>
```

| Input | Result |
|-------|--------|
| omitted, `read committed` | `ReadCommitted` |
| `repeatable read`, `snapshot` | `Snapshot` |
| `read uncommitted` | `ReadCommitted` (documented upgrade) |
| `serializable` | refuse `serializable_nongoal` |

`003` `TypeDescriptor.snapshot_capable: bool`. SNAPSHOT + any touched type with `false` → `snapshot_unsupported{type}`. Mixed-type txn including a non-capable type: same refuse. Cassandra consistency is **not** stored here (`012`).

## 5. Limits

```text
struct Limits {
    max_key: ByteCount,                          // default 1 KiB
    max_value: ByteCount,                        // default 16 MiB
    max_query_text: ByteCount,                   // default 1 MiB
    max_result: ByteCount,                       // default 64 MiB
    max_connections_per_entrypoint: u32,         // default 10_000
    max_connections_per_principal: u32,          // default 1_000
}

enum LimitKind {
    Key, Value, QueryText, Result,
    ConnectionsEntrypoint, ConnectionsPrincipal,
    ConcurrentQueriesNode, ConcurrentQueriesNamespace,
    QueryMemory, Buffer, SpillDisk,
}
```

Concurrent-query and query-memory knobs live in `005` `query { }` with defaults 512 / 128 / 256 MiB. This struct does not duplicate them; `LimitKind` is the named-error vocabulary for both.

Zero or missing-required → startup/validate error. Live-reload: new connections/queries only.

## 6. AdmissionPolicy

```text
struct AdmissionPolicy {
    spill: SpillMode,            // FirstBinary/HandlersComplete: Off; CompleteProduct: On
}

enum SpillMode { Off, On }
```

Acquire: node concurrent slot → namespace concurrent slot → memory reservation. Failure: `admission_rejected{limit, current, max}`. Never hang. Spill behaviour: [limits.md](contracts/limits.md).

## 7. ProductVersion window

```text
struct ProductVersion(u16);      // first binary = 1

fn peers_ok(local: ProductVersion, peer: ProductVersion) -> bool {
    local.0.abs_diff(peer.0) <= 1
}
```

N+2 → refuse join/internode. N node: refuse create when `TypeDescriptor.introduced_in > local`; refuse write of disk format > local (`013`). First-binary conformance: all nodes `ProductVersion(1)`.

## 8. Session counters (process memory)

| Field | Scope |
|-------|--------|
| connections_on_entrypoint | per listener |
| connections_of_principal | per `PrincipalId` (`014`) |
| inflight_queries_node | node |
| inflight_queries_namespace | namespace |
| reserved_query_memory | per exec |

Not Raft. Lost on restart (in-flight queries do not survive anyway).

## 9. Named errors (validation / runtime)

| Code | When |
|------|------|
| `compat_must_not{protocol,verb}` | classify MustNot |
| `serializable_nongoal` | SERIALIZABLE requested |
| `snapshot_unsupported{type}` | SNAPSHOT on non-capable type |
| `limit_exceeded{limit,current,max}` | size/connection |
| `admission_rejected{limit,current,max}` | concurrent query / memory / spill disk / buffer |
| `product_version_window{local,peer}` | peer outside N/N+1 |
| `format_too_new{have,need}` | N node would write N+1 format (`013`) |
| `type_too_new{type,introduced_in,local}` | N node creates N+1 type |
| `limits_zero{knob}` | config 0 |
| `copy_not_in_profile` | COPY on FirstBinary / HandlersComplete |
| `begin_not_in_profile` | BEGIN on FirstBinary / HandlersComplete |
| `cursor_not_in_profile` | SQL DECLARE on FirstBinary / HandlersComplete |
| `agg_not_in_profile` | ES aggregation before slice 8 |

Wire encoding: [R10](research.md) / `002` renderer.

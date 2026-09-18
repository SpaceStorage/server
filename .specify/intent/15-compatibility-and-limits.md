---
speckit_command: specify
suggested_slug: compatibility-and-limits
source: gap-analysis-2026-09-14
read_after: 00-constitution.md
---

# Feature: Protocol compatibility subsets, query/transaction limits, and rolling upgrade

Specify what "implement PostgreSQL, Cassandra, Redis, Elasticsearch, ClickHouse, S3, and WebDAV" means in a finite product: wire versions, must-implement vs explicit non-goals, isolation levels, size/connection limits, and mixed-version clusters. Without this file, implementers either fake a subset (forbidden) or never finish.

## What

The constitution requires all listed protocols. This feature defines the **compatibility contract** each protocol MUST meet, and the **non-goals** that MUST return the protocol's native "not supported" error (never a silent empty success).

ClickHouse is two handlers (`clickhouse` native and `clickhouse-http`), both required (`02`).

### Wire versions (must speak)

- PostgreSQL: frontend/backend protocol 3.0 (SSLRequest, Startup, simple + extended query)
- Cassandra: native protocol v4 and v5 (OPTIONS/STARTUP/QUERY/EXECUTE/PREPARE/BATCH/REGISTER as applicable)
- Redis: RESP2; AUTH, and the mapped command set below. RESP3 MAY be added later
- Elasticsearch: HTTP/1.1 REST subset documented below (not the full ES product)
- ClickHouse: native protocol Hello/query/data and HTTP interface query/insert
- S3: SigV4-capable REST; ListBuckets, CreateBucket, Put/Get/DeleteObject, multipart upload, ListObjectsV2
- WebDAV: RFC 4918 subset: PROPFIND, GET, PUT, DELETE, MKCOL, MOVE, COPY

Handshake mismatch: refuse within one second in the protocol's own refusal form (`02`).

### Must-implement vs complete-product non-goals

This matrix is the **complete-product** compatibility ceiling (`00`). It is **not** a "v1" ship list. The **first binary** implements only PostgreSQL and Redis, and only the subsets in `16`. Cassandra, Elasticsearch, ClickHouse, S3, and WebDAV handlers are **not** in the first binary.

Every type in the inventory remains **visible** through every **complete-product** protocol (`02`/`03`). Non-goals are **product features of the emulated system**, not SpaceStorage types. Stock **clients** on MUST verbs are in scope; unmodified **applications** that need MUST-NOT verbs are not.

| Protocol | Complete-product MUST | Complete-product MUST NOT (not-supported error) |
|----------|-----------------------|--------------------------------------------------|
| PostgreSQL | Simple+extended query; BEGIN/COMMIT/ROLLBACK; INSERT/UPDATE/DELETE/SELECT; CREATE/ALTER/DROP mapped to containers; COPY; prepared statements; `search_path` / database name as namespace | PL/pgSQL, stored procedures, triggers, LISTEN/NOTIFY, logical/physical replication protocol, FDW, extensions, cursors beyond a documented holdable subset |
| Cassandra | CQL DML/DDL for mapped types; consistency level; prepared statements; keyspace as namespace | Cassandra LWT as a second transaction model (use SpaceStorage transactions `05`); native Cassandra materialized views; Cassandra CDC |
| Redis | GET/SET/DEL/EXISTS/SCAN/PING/AUTH/SELECT-as-no-op-inside-bound-namespace; TTL commands mapped to container TTL; documented commands for type-specific ops | Redis Cluster hash slots, Redis modules, Lua, Redis Streams as a separate product (Log Stream is an L3 type) |
| Elasticsearch | Index/get/delete document; create index with mappings; search (query_string / documented subset); cat/list | ILM, ingest pipelines, ML, CCR, full aggregations beyond what `05` implements |
| ClickHouse | CREATE TABLE / INSERT / SELECT with WHERE and GROUP BY mapped to the execution layer | Dictionaries, ClickHouse-native replication, ClickHouse materialized views (use L4) |
| S3 | Bucket and object CRUD, multipart | Bucket versioning, object lock, S3 replication, S3 Select, ACLs beyond SpaceStorage authz |
| WebDAV | Collection and file CRUD, MOVE/COPY | LOCK/UNLOCK as a distributed lock product; CalDAV/CardDAV |

First-binary PostgreSQL and Redis **subsets** (narrower than this table) live in `16`. Adding a verb to MUST or MUST NOT here is a spec change of this feature.

### Query language and transactions (`05` consumes this)

- SQL dialect for PostgreSQL and ClickHouse handlers: **PostgreSQL-shaped SQL** for PG; ClickHouse SQL for `clickhouse` / `clickhouse-http`, both planned into one canonical execution IR owned by `05`.
- Isolation levels the planner MAY attach: `READ COMMITTED` (default for SQL) and `SNAPSHOT` where the type supports it. **`SERIALIZABLE` is a product non-goal** (not "later"). There is no third isolation level to attach.
- Cassandra-style consistency (`ONE` / `LOCAL_ONE` / `TWO` / `QUORUM` / `EACH_QUORUM`) is **not** SQL isolation. Write/read quorum arithmetic, including `LOCAL_*` (coordinator's domain) and `EACH_QUORUM` (source quorum plus async remote applied), is `12`.
- Prepared statements and COPY as above (complete product). EXPLAIN MUST exist on SQL protocols (logical plan at least). First-binary PostgreSQL has **no COPY and no BEGIN** (`16`).
- Query cancellation: client disconnect or explicit cancel MUST stop work within the timeout window (`02` SC-005 class).
- Admission: documented max query memory; spill to disk or refuse; documented max concurrent queries per node and per namespace (`07` quotas).

### Limits (documented defaults; administrator MAY raise within bounds)

- Max key size, max value/document/object size, max query text size, max result size (histogram already in `08`)
- Max connections per entrypoint and per principal
- Behavior at buffer-full / quota: **reject** the offending request with a named error (not throttle-by-hang). Soft warning metrics MAY exist; enforcement is hard at the documented limit.

Quota **units** (`07`): bytes stored, object/row counts, connections, and (optional) IOPS-equivalent operation counts, per namespace and per data type. CPU isolation between tenants is a product non-goal for the first binary and remains best-effort fairness in the complete product unless later amended.

### Rolling upgrade

On-disk format version is `13`. Product versions: a cluster MAY run mixed **N and N+1** only. Feature gates: an N node MUST refuse to create container types or format versions introduced in N+1. Catalog-diff across releases is reportable (`03`).

## Why

"Work over seven protocols" is not a testable requirement. Isolation "if needed" is not a testable requirement. Size limits that exist only as metrics will OOM production.

## Actors

- Application using a stock client within the MUST subset
- Developer hitting a MUST NOT verb and receiving a clear not-supported error
- Operator rolling N → N+1
- Planner attaching isolation and admission (`05`)
- Tenant hitting a quota (`07`)

## Requirements

- Wire versions listed above.
- MUST / MUST NOT matrix; not-supported is explicit.
- Isolation levels: READ COMMITTED (default) and SNAPSHOT where the type supports it; SERIALIZABLE is a non-goal.
- Cancellation, EXPLAIN on SQL, prepared statements/COPY as in the complete-product matrix; first-binary PG/Redis in `16`.
- Quorum vocabulary (`LOCAL_*`, `EACH_QUORUM`) points at `12`.
- Documented size and connection limits; hard reject at limit.
- Mixed-version N/N+1; old nodes refuse new formats/types.

## Out of scope for this feature

- Abstract datatype interface (`02`/`03`)
- Placement and quorum (`04`)
- Full execution engine inventory (`05`) except dialect/isolation/admission contracts
- Auth mechanisms (`14`)
- MVP sequencing (`16`) — this file is the compatibility *ceiling* of a complete product; `16` is what ships first

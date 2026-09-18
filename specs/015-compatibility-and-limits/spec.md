# Feature Specification: Protocol Compatibility Ceiling, Limits, Isolation, and Rolling Upgrade

**Feature Branch**: `015-compatibility-and-limits`

**Created**: 2026-09-15

**Status**: Draft

**Input**: User description: "Read .specify/intent/15-compatibility-and-limits.md and specify this feature." — complete-product MUST/MUST NOT verb matrix and wire versions; isolation `READ COMMITTED` / `SNAPSHOT` only (`SERIALIZABLE` non-goal); size/connection limits with hard reject; mixed-version N/N+1. This file is the compatibility **ceiling**, not the first binary (`16`).

## Clarifications

### Session 2026-09-15

- Q: Is this matrix "v1"? → A: No. It is the complete-product ceiling. Do not brand the seven-protocol matrix as "v1". First binary is `16` (PostgreSQL + Redis subsets only).
- Q: Stock clients or stock applications? → A: Unmodified **clients** (psql, redis-cli, …) on the MUST verb list. Unmodified **applications** that need MUST-NOT verbs are out of scope.
- Q: Is `SERIALIZABLE` later? → A: No. Product non-goal.

### Session 2026-09-18

- Q: When a query’s working set goes past the documented max query memory, must the product spill to disk, or is refusing the query always enough? → A: Concurrent-query caps always reject. Query memory: first binary always rejects. Complete product MUST spill for documented sort/hash/aggregation plans; if that plan cannot spill, reject naming the cap. Never hang.
- Q: When a SQL client asks for `SNAPSHOT` isolation, which container types honor it, and what happens if the type cannot? → A: Types `03` marks snapshot-capable honor `SNAPSHOT`. PostgreSQL `REPEATABLE READ` maps to `SNAPSHOT`. A request that touches a non-capable type, or a mixed-type transaction that includes one, is refused naming the type and the allowed set (`READ COMMITTED`, `SNAPSHOT`). No silent downgrade. First binary has no `BEGIN`, so this is complete-product.
- Q: Which PostgreSQL cursor verbs must work in the complete product, given the MUST NOT column currently says “cursors beyond a documented holdable subset” but never documents that subset? → A: Complete-product MUST is forward-only `DECLARE` / `FETCH` / `CLOSE` inside an open transaction (no `WITH HOLD`). `WITH HOLD`, scrollable cursors, and portals that survive `COMMIT` are MUST NOT. First binary: not required (no `BEGIN`; extended-query prepared portals remain as in `16`).
- Q: Must a mixed-version cluster (product N talking to N+1, and N refusing N+1 types/formats) already work in the first shippable binary, or only in the complete product? → A: Mixed N/N+1 (and N+2 refuse) is complete-product only. First binary: every node the same product version. Size/connection/admission limits in that binary still hard-reject.
- Q: Which Elasticsearch search and aggregation requests must succeed, and which must return not-supported, without pointing at “whatever the query engine implements”? → A: MUST search is `query_string`, `match`, `term`, `range`, and `bool` filter. MUST aggregations are the closed list `terms`, `min`/`max`/`sum`/`avg`, `histogram`, `value_count`. ILM, ingest pipelines, ML, CCR, and any aggregation not in that list are MUST NOT. The list lives in this spec; `05` implements it. Document CRUD + search ship with the ES handler (slice 6); aggregations wait for slice 8. No ES handler in the first binary.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Complete-product verb contract (Priority: P1)

An application team uses a stock client. Verbs in the MUST column work. Verbs in the MUST NOT column return that protocol's not-supported error, never a silent empty success. ClickHouse is two handlers. Handshake mismatch refuses within one second (`02`).

**Why this priority**: "Work over seven protocols" was untestable without this matrix.

**Independent Test**: For each complete-product handler, run MUST smoke; issue one MUST-NOT verb; confirm not-supported. PostgreSQL: forward-only `DECLARE`/`FETCH`/`CLOSE` inside a transaction; `WITH HOLD` not-supported. Elasticsearch: document CRUD + MUST search; aggregations after slice 8; ILM not-supported.

**Acceptance Scenarios**:

1. **Given** the complete-product PostgreSQL handler, **When** a stock client speaks protocol 3.0 with simple + extended query, BEGIN/COMMIT, DML/DDL mapped to containers, COPY, prepared statements, database name as namespace, and forward-only `DECLARE`/`FETCH`/`CLOSE` inside an open transaction, **Then** those succeed. PL/pgSQL, LISTEN/NOTIFY, FDW, extensions, `WITH HOLD`, scrollable cursors, and portals that survive `COMMIT` return not-supported.
2. **Given** Cassandra native v4/v5, **When** CQL DML/DDL, consistency level, prepared statements, and keyspace-as-namespace are used, **Then** they succeed. Cassandra LWT-as-second-transaction-model, native Cassandra MVs, Cassandra CDC return not-supported.
3. **Given** Redis RESP2, **When** AUTH, PING, GET/SET/DEL/EXISTS/SCAN, SELECT-as-no-op inside the bound namespace, and TTL mapped to container TTL are used, **Then** they succeed. Redis Cluster slots, modules, Lua, Redis Streams-as-product return not-supported.
4. **Given** Elasticsearch HTTP REST, **When** index/get/delete document, create index with mappings, cat/list, and search with `query_string` / `match` / `term` / `range` / `bool` filter run, **Then** they succeed. **Given** slice 8, **When** aggregations `terms`, `min`/`max`/`sum`/`avg`, `histogram`, or `value_count` run, **Then** they succeed through the shared engine (`05`). ILM, ingest pipelines, ML, CCR, and any aggregation not in that list return not-supported. **Given** ClickHouse native+HTTP, S3 SigV4 CRUD+multipart+ListObjectsV2, WebDAV RFC 4918 subset, **When** MUST verbs run, **Then** they succeed; MUST NOT verbs (CH dictionaries, S3 versioning, LOCK/UNLOCK as a lock product, …) return not-supported.
5. **Given** a handshake mismatch, **When** a client speaks the wrong protocol, **Then** refusal is within one second in that protocol's form (`02`).

---

### User Story 2 - Isolation, cancel, admission (Priority: P1)

SQL isolation is `READ COMMITTED` (default) or `SNAPSHOT` on types `03` marks snapshot-capable. PostgreSQL `REPEATABLE READ` maps to `SNAPSHOT`. A `SNAPSHOT` request that touches a non-capable type, or a mixed-type transaction that includes one, is **refused** naming the type and the allowed set — never silently applied as `READ COMMITTED`. `SERIALIZABLE` is refused. Cassandra-style consistency is not SQL isolation (`12`). Client disconnect or cancel stops work. Max concurrent queries per node and per namespace are documented; overflow **rejects** with a named error (never hang). Max query memory is documented: the **first binary** always **rejects** when the cap is exceeded; the **complete product** MUST **spill** for documented sort, hash, and aggregation plans, and MUST **reject** naming the cap when that plan cannot spill. Quota units consumed here are defined in `07`.

**Why this priority**: Isolation "if needed" was untestable; grill closed SERIALIZABLE.

**Independent Test**: BEGIN at default isolation; SET SERIALIZABLE (refuse); SNAPSHOT / REPEATABLE READ on a snapshot-capable type; SNAPSHOT on a non-capable type (refuse, no downgrade); cancel; hit concurrent-query max (reject). First binary: over-memory query rejects; no BEGIN. Complete product: sort/hash/agg under spill completes; a plan that cannot spill rejects naming the cap.

**Acceptance Scenarios**:

1. **Given** SQL with no isolation, **When** a transaction runs, **Then** applied isolation is `READ COMMITTED`.
2. **Given** `SERIALIZABLE`, **When** it is requested, **Then** it is refused as a non-goal.
2a. **Given** complete-product SQL and a snapshot-capable type (`03`), **When** `SNAPSHOT` or PostgreSQL `REPEATABLE READ` is requested, **Then** statements in that transaction see a consistent snapshot as documented for that type.
2b. **Given** `SNAPSHOT` (or `REPEATABLE READ`) on a type `03` does not mark snapshot-capable, or a transaction that also touches such a type, **When** it is requested, **Then** it is refused naming the type and the allowed set (`READ COMMITTED`, `SNAPSHOT`); applied isolation MUST NOT silently become `READ COMMITTED`.
3. **Given** disconnect or cancel, **When** it occurs, **Then** work stops within the timeout window (`02`/`05`).
4. **Given** max concurrent queries exceeded on a node or namespace, **When** the extra query arrives, **Then** it is rejected with a named error, not hung (first binary and complete product).
5. **Given** first binary and a query whose working set exceeds max query memory, **When** it runs, **Then** it is rejected naming the cap; no spill is required.
6. **Given** complete product and a documented sort, hash, or aggregation plan, **When** the working set exceeds max query memory, **Then** it completes using spill. **Given** a plan that cannot spill, **When** the cap is exceeded, **Then** it is rejected naming the cap, not hung.

---

### User Story 3 - Size limits and rolling upgrade (Priority: P2)

Documented maxima: key, value/document/object, query text, result size, connections per entrypoint and per principal. At the limit, reject with a named error. These size and connection limits apply in the **first binary** and the complete product.

**Rolling upgrade is complete-product only** (`16` defers it). Mixed product versions N and N+1 only. An N node refuses types or format versions introduced in N+1 (`13` format version). Catalog-diff across releases is reportable (`03`). N+2 MUST NOT join (`12`). The **first binary** is same-version: every node the same product version; mixed-cluster fixtures are not required for slices 1–5.

**Why this priority**: Limits that exist only as metrics will OOM production. Mixed-version without a format version is untestable in the first binary.

**Independent Test**: Oversized value (first binary). Complete product: mixed N/N+1 cluster; N node create of N+1 type; N+2 join refused.

**Acceptance Scenarios**:

1. **Given** a value over the documented max (first binary or complete product), **When** it is written, **Then** it is rejected naming the limit.
2. **Given** buffer-full or quota, **When** the offending request arrives, **Then** it is rejected with a named error (`01`/`07`); soft warning metrics MAY exist.
3. **Given** a complete-product cluster on N and N+1, **When** they interoperate, **Then** it is allowed; N+2 MUST NOT join (`12` version window).
4. **Given** a complete-product N node, **When** it would create an N+1 type or write an N+1 format, **Then** it refuses.
5. **Given** the first binary, **When** the cluster is brought up, **Then** every node is the same product version; mixed N/N+1 fixtures MUST NOT be required.

---

### Edge Cases

- First-binary PostgreSQL/Redis subsets in `16` are **narrower** than this table; adding a verb to MUST or MUST NOT here is a spec change of this feature.
- CPU hard isolation is a product non-goal; fairness is best-effort.
- EXPLAIN MUST exist on SQL protocols (logical plan at least) for the complete product; first binary MAY still offer EXPLAIN (`05`).
- Query-memory spill is complete-product (sort/hash/aggregation plans, typically slice 8 / `05`). First-binary CRUD MUST NOT require spill. Concurrent-query admission never spills.
- `SNAPSHOT` / `REPEATABLE READ` on a non-capable type: refuse, no downgrade. Which types are snapshot-capable is `03`; this feature owns the refuse contract.
- First-binary PostgreSQL has no `BEGIN`, so `SNAPSHOT` is not a first-binary requirement.
- PostgreSQL cursor subset (this is the documented holdable subset named in the intent): complete-product forward-only `DECLARE`/`FETCH`/`CLOSE` inside a transaction. `WITH HOLD`, scrollable, and hold-across-commit are MUST NOT. First binary MUST NOT require SQL `DECLARE`.
- Mixed N/N+1 is complete-product. First binary MUST NOT require two product versions in one cluster.
- Elasticsearch: no handler in the first binary. Slice 6: document CRUD + MUST search. Slice 8: the closed aggregation list. Aggregations before slice 8 return not-supported.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Wire versions MUST be: PostgreSQL frontend/backend 3.0; Cassandra native v4 and v5; Redis RESP2 (RESP3 MAY later); Elasticsearch HTTP/1.1 REST subset; ClickHouse native Hello/query/data and HTTP query/insert; S3 SigV4 REST with ListBuckets, CreateBucket, Put/Get/DeleteObject, multipart, ListObjectsV2; WebDAV RFC 4918 subset PROPFIND, GET, PUT, DELETE, MKCOL, MOVE, COPY.
- **FR-002**: ClickHouse MUST be two handlers (`clickhouse`, `clickhouse-http`), both required in the complete product (`02`).
- **FR-003**: The MUST / MUST NOT matrix in the intent file is the complete-product ceiling, with the PostgreSQL cursor line replaced by FR-014 and the Elasticsearch search/aggregation line replaced by FR-015 (this spec defines those subsets; they are not “whatever `05` implements”). Verbs outside MUST MUST return that protocol's not-supported error, never a silent empty success.
- **FR-004**: This matrix MUST NOT be labeled or planned as the first shippable binary. First binary subsets live in `16`.
- **FR-005**: Stock **clients** on MUST verbs are in scope; unmodified **applications** that need MUST-NOT verbs are not.
- **FR-006**: Isolation the planner MAY attach: `READ COMMITTED` (SQL default) and `SNAPSHOT` on types `03` marks snapshot-capable. PostgreSQL `SET TRANSACTION ISOLATION LEVEL REPEATABLE READ` MUST map to `SNAPSHOT`. `SNAPSHOT` / `REPEATABLE READ` that touches a type not so marked, or a mixed-type transaction that includes one, MUST be refused naming the type and the allowed set (`READ COMMITTED`, `SNAPSHOT`); it MUST NOT silently become `READ COMMITTED`. `SERIALIZABLE` MUST be refused as a product non-goal. This isolation contract is complete-product; first-binary PostgreSQL has no `BEGIN` (`16`).
- **FR-007**: Cassandra-style consistency is not SQL isolation; arithmetic is `12`.
- **FR-008**: Prepared statements, COPY, EXPLAIN (logical plan at least on SQL), and the PostgreSQL cursor subset in FR-014 MUST exist for the complete-product MUST subset. First-binary PostgreSQL has no COPY, no BEGIN, and no SQL `DECLARE` (`16`).
- **FR-009**: Client disconnect or explicit cancel MUST stop work within the timeout window.
- **FR-010**: Documented max concurrent queries per node and per namespace (`07`): exceeding the cap MUST **reject** the extra query with a named error (never hang). Documented max query memory: the **first binary** MUST **reject** when the working set exceeds the cap (spill MUST NOT be required). The **complete product** MUST **spill** for documented sort, hash, and aggregation plans; when that plan cannot spill, it MUST **reject** naming the cap. Spill MUST NOT be a substitute for concurrent-query admission.
- **FR-011**: Documented max key, value/document/object, query text, result size, connections per entrypoint and per principal. Enforcement at limit is **reject** with a named error.
- **FR-012**: Quota units remain `07` (bytes, counts, connections, optional operation counts). CPU hard isolation MUST NOT be claimed.
- **FR-013**: Complete product: mixed product versions N and N+1 only. N MUST refuse types/formats introduced in N+1. Catalog-diff MUST be reportable (`03`). On-disk format version is `13`. N+2 MUST NOT join (`12`). The **first binary** MUST run a single product version on every node; mixed N/N+1 and N+2-join tests MUST NOT be required for slices 1–5 (`16`). Size, connection, and admission limits (FR-010, FR-011) still apply in that binary.
- **FR-014**: Complete-product PostgreSQL MUST implement forward-only `DECLARE`, `FETCH`, and `CLOSE` inside an open transaction (no `WITH HOLD`). `WITH HOLD`, scrollable cursors, and portals that remain usable after `COMMIT` MUST return that protocol's not-supported error. First-binary PostgreSQL MUST NOT require SQL `DECLARE`/`FETCH`/`CLOSE`; extended-query prepared portals remain as in `16`.
- **FR-015**: Elasticsearch MUST search is `query_string`, `match`, `term`, `range`, and `bool` filter, plus index/get/delete document, create index with mappings, and cat/list. MUST aggregations are the closed list `terms`, `min`, `max`, `sum`, `avg`, `histogram`, and `value_count`; `05` implements that list and MUST NOT define it. ILM, ingest pipelines, ML, CCR, and any aggregation not in that list MUST return not-supported. Document CRUD and MUST search ship with the ES handler (slice 6). MUST aggregations wait for slice 8 (`16`); before that they MUST return not-supported. The first binary MUST NOT include an Elasticsearch handler.

### Key Entities

- **Compatibility Matrix**: Complete-product MUST vs MUST NOT per protocol; PostgreSQL cursor subset is FR-014; Elasticsearch search/aggregation subset is FR-015.
- **Wire Version**: Documented handshake each handler speaks.
- **Isolation Set**: `READ COMMITTED` (SQL default); `SNAPSHOT` on `03` snapshot-capable types (`REPEATABLE READ` maps to it); `SERIALIZABLE` excluded. Non-capable types: refuse, no downgrade.
- **Admission Limit**: Concurrent queries (reject at cap), query memory (first binary reject; complete-product spill for sort/hash/agg else reject), sizes, connections.
- **Product Version Window**: Complete-product N / N+1 (N+2 refused). First binary: same version on every node.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of MUST-column smoke workflows for each complete-product handler succeed with a stock client, including complete-product PostgreSQL forward-only `DECLARE`/`FETCH`/`CLOSE` inside a transaction. Slice 6: 100% of ES document CRUD and MUST-search cases in the suite succeed. Slice 8: 100% of the closed ES aggregation list in the suite succeed.
- **SC-002**: 100% of MUST-NOT verbs in the suite return that protocol's not-supported form; 0 silent empty successes. Complete-product PostgreSQL: 100% of `WITH HOLD`, scrollable, and hold-across-commit attempts in the suite are not-supported. Elasticsearch: 100% of ILM, ingest pipelines, ML, CCR, and aggregations outside the closed list in the suite are not-supported; before slice 8, 100% of aggregations in the suite are not-supported.
- **SC-003**: 100% of `SERIALIZABLE` requests are refused. Complete product: 100% of `SNAPSHOT` / `REPEATABLE READ` requests on snapshot-capable types in the suite apply snapshot isolation; 100% of such requests that touch a non-capable type are refused naming the type; 0 silent downgrades to `READ COMMITTED`.
- **SC-004**: 100% of oversize, over-connection, and over-concurrent-query requests reject with the limit named; 0 hangs. First binary: 100% of over-memory queries in the suite reject naming the cap. Complete product: 100% of documented sort/hash/aggregation over-memory cases in the suite complete via spill; 100% of non-spillable over-memory plans reject naming the cap; 0 hangs.
- **SC-005**: First binary: 100% of cluster fixtures in the suite are same product version. Complete product: 100% of N nodes refuse N+1 types/formats; N/N+1 clusters interoperate in 100% of mixed-version tests; 100% of N+2 join attempts are refused.

## Assumptions

- Protocol adapters (`02`) enforce not-supported on the wire. Execution (`05`) consumes isolation/admission/dialects and implements the Elasticsearch aggregation list defined here. Type snapshot-capability flags are `03`.
- First binary (`16`) is a **subset** of this ceiling, not a contradiction of the constitution (complete product still owes all handlers). Mixed-version rolling upgrade is owed by the complete product of this feature, not by slices 1–5.
- Default numeric limits are planning decisions; existence of the limits is not. Concurrent-query and size/connection enforcement is hard-reject. Query-memory enforcement is hard-reject in the first binary; complete-product sort/hash/aggregation MUST spill, else reject.

## Out of Scope

- Abstract datatype interface (`02`/`03`).
- Placement and replica targeting (`04`).
- Full execution engine inventory (`05`) except dialect/isolation/admission contracts.
- Auth mechanisms (`14`).
- MVP sequencing (`16`) — this file is the ceiling; `16` is what ships first.

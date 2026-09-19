# Feature Specification: Query Execution, MapReduce, Transactions, and Fault-Tolerant Results

**Feature Branch**: `005-query-execution`

**Created**: 2026-09-14

**Updated**: 2026-09-16

**Status**: Ready for implement

**Input**: User description: "005-query-execution" — re-specify the shared execution layer from `.specify/intent/05-query-execution.md` after constitution 1.3.0 and features `011`–`016`: parser, planner, task scheduler, task executor, MapReduce, shuffle, aggregation, join, data exchange, and execution state, working natively for all complete-product protocols through one canonical intermediate representation; planner-owned concurrency and a closed isolation set (`READ COMMITTED`, `SNAPSHOT` where the type supports it; **`SERIALIZABLE` is a product non-goal**); timeout and quorum on every query; subscribe-on-results; cancel on disconnect; explicit unavailability when a data part has zero live replicas; shuffle over the `internode` fabric; replica/shuffle locality by topology ladder then measured RTT and HLC-skew health (`04`/`12`); first-binary PostgreSQL/Redis CRUD through this stack without `COPY`/`BEGIN` (`16` slices 1–5); joins/aggregation/MapReduce/subscribe/distributed transactions as slice 8.

## Clarifications

### Session 2026-09-15

- Q: Is `SERIALIZABLE` in the closed isolation set? → A: No. Product non-goal (`15`/`16`). Allowed: `READ COMMITTED` (SQL default) and `SNAPSHOT` where the type supports it.
- Q: How are replicas and shuffle workers ranked? → A: Topology ladder (first differing key = farther), then measured internode RTT (`04`/`12`). `LOCAL_*` is the coordinator's `quorum_domain`.
- Q: Are COPY and BEGIN required in the first binary? → A: No. Complete-product PostgreSQL MUST includes them (`15`). First-binary PostgreSQL is auto-commit only, no COPY, no BEGIN (`16`). This stack still owns them when the MUST subset includes them.

### Session 2026-09-16

- Q: What does this stack owe the first shippable binary versus the complete product? → A: Slices 1–5 (`16`) run auto-commit PostgreSQL DML/DDL plus extended/prepared, and the Redis MUST list on `K/V Store`, through this stack — timeout, quorum, cancel, unavailability, admission. Slice 8 is query beyond CRUD: joins, aggregation, MapReduce, subscribe, distributed transactions. `BEGIN`/`COPY` remain complete-product (`15`), not first-binary.
- Q: Does replica ranking use only RTT? → A: Rank by the first topology-ladder key that differs (near before far). Measured internode RTT, and HLC skew as a health signal, MUST override that rank once samples exist (`04` FR-001b, `12` FR-018). HLC MUST NOT be compared across `quorum_domain`s. A missing ladder value is not local.
- Q: Who forwards a write that lands in a log-follower domain? → A: This execution layer, as coordinator. The write forwards to the source domain; `LOCAL_*` write still requires a durable acknowledgement in the source (`12`). `EACH_QUORUM` is refused unless the container opted in (`04` FR-074).
- Q: When a client starts a long job and wants the result later without keeping that original request open, how should they wait for it? → A: Same job object as admin, plus a documented query that returns a job id and a poll/wait query in SQL/CQL (not `LISTEN`/`NOTIFY`). Redis and HTTP use a documented command or path.
- Q: When some of the data a query needs is unreachable, how should a client ask to receive the reachable part instead of failing the whole query? → A: Documented per-query or per-session option in the same family as timeout and quorum (and a protocol-native field where one exists). Default off: missing shards fail the whole query.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Run a native protocol query through one shared planner (Priority: P1)

An application issues a statement through a stock client. In the first shippable binary that is PostgreSQL (simple and extended/prepared, auto-commit DML/DDL) or Redis (MUST list on `K/V Store`). In the complete product it is also CQL, Elasticsearch search, ClickHouse SQL, S3 GET/PUT, or WebDAV GET/PUT. The protocol handler translates the request into the single execution representation this feature owns. The planner matches the operation to the container's type (`03`), attaches timeout and quorum (`02`/`04`), and the executor returns a result in that protocol's shape. Two semantically equivalent queries through different protocols over the same data produce the same logical result. No protocol keeps a private engine.

**Why this priority**: Without a shared execution layer, each protocol becomes a second engine, which the constitution and `16` forbid. This is the first slice that makes "native for all protocols" testable; the first binary proves it on PostgreSQL and Redis.

**Independent Test**: On one node, create a `Relational Table` and a `K/V Store`. Run first-binary PostgreSQL smoke (`16`: INSERT/SELECT/UPDATE/DELETE, CREATE/DROP, prepared; no COPY, no BEGIN) and Redis MUST-list commands on KV. Compare logical content for a value written through one and read through the other. Delivers a working planner/executor with no MapReduce, no `BEGIN`, and no distributed transactions required.

**Acceptance Scenarios**:

1. **Given** a namespace with a `Relational Table` container, **When** a PostgreSQL client runs `INSERT` then `SELECT` in the first-binary subset (`16`), **Then** the rows round-trip unchanged and the execution record names the shared planner (not a protocol-private engine).
2. **Given** the same rows stored so a Redis mapping applies, or a `K/V Store` written through Redis, **When** the other protocol reads the documented mapping (`02`), **Then** the logical values match.
3. **Given** a query whose verb is in the protocol's MUST NOT column (`15`), or COPY/BEGIN on first-binary PostgreSQL (`16`), **When** it is submitted, **Then** the client receives that protocol's not-supported error, nothing is planned, and nothing is stored.
4. **Given** an operation the container type does not support (for example nearest-neighbour search on a `Log Stream`), **When** it is planned, **Then** it is refused before touching data, naming the type and the operation (`03`).
5. **Given** EXPLAIN on a SQL protocol, **When** a client asks for the logical plan of a `SELECT`, **Then** a plan is returned without executing the query, and it names the containers and operations involved. First binary MAY offer EXPLAIN; complete-product SQL MUST (`15`).
6. **Given** a complete-product build with additional handlers (`16` slice 6), **When** MUST-subset verbs for Cassandra, Elasticsearch, ClickHouse, S3, and WebDAV run, **Then** they execute through the same planner and IR as PostgreSQL and Redis.

---

### User Story 2 - Timeout, quorum, cancel, unavailability, and domain-aware coordination (Priority: P1)

Every query carries a timeout and a quorum level. If the client omits them, documented defaults apply: a default that exceeds replica count is **clamped** and reported; an **explicit** unsatisfiable level is **rejected** before execution (`02`). If the client disconnects or sends a cancel, work stops. If every replica of a needed data part is down, the client is told that part is unavailable — not given a silent empty success, and not given a partial result unless the **partial-results option** is on (same option family as timeout and quorum: per query, per session, or a protocol-native field where one exists; default **off**). A write that lands on a log-follower is forwarded to the source. Multi-shard work is fanned out with quorum applied per shard.

**Why this priority**: Constitution and `02`/`04`/`12` already require timeout, quorum, and source-domain durability; this feature is where they are enforced during execution. Fault-tolerance wording in the intent is otherwise unimplemented.

**Independent Test**: Three-node cluster with a replicated container in one `quorum_domain`: run queries at `ONE` and `TWO`, stop all replicas of one shard, cancel a long query, and let a timeout fire. Optionally place a log-follower in a second domain and confirm writes forward. Delivers the execution contract without MapReduce.

**Acceptance Scenarios**:

1. **Given** a query with no timeout or quorum, **When** it executes, **Then** the applied options are the global (or container) defaults and are inspectable on the execution record; a default write `TWO` on a single replica is clamped to `ONE` and reported as clamped.
2. **Given** an explicit quorum the container cannot satisfy, **When** the query is submitted, **Then** it is rejected before touching data, naming the requested level and the available replica count.
3. **Given** a timeout of T, **When** T elapses, **Then** the client receives the protocol's timeout error within one second of the deadline and the query stops consuming resources within that window. T itself is configurable and is not fixed on the assumption of a low round trip (`04` FR-073).
4. **Given** a client disconnect or explicit cancel, **When** it occurs mid-query, **Then** the query is stopped within the same window as timeout and is recorded as cancelled.
5. **Given** a shard whose replicas are all down and the partial-results option is off (the default), **When** a query needs that shard, **Then** the client is told that part of the data is not available now, the unavailable part is named, and the query fails as a whole.
5a. **Given** the same shard-down situation and the partial-results option on (per query, per session, or protocol-native field), **When** the query runs, **Then** reachable shards are returned together with the named unavailable part; the client is not told the result is complete.
6. **Given** a write whose quorum cannot be met, **When** it is executed, **Then** the client is not told the write succeeded (`04`). Persistent and hybrid acknowledgements wait for the durability contract (`13`), not an in-memory write alone.
7. **Given** a query that spans several shards, **When** it runs, **Then** the coordinator fans it out, applies quorum per shard, and a shard that misses quorum fails the query naming the shard and the level unless the partial-results option is on (`04` FR-053).
8. **Given** a coordinator in a log-follower domain, **When** a write is submitted, **Then** it is forwarded to the source; `LOCAL_ONE` write still requires one durable source acknowledgement; `LOCAL_ONE` read MAY be served from a local applied replica and MAY be stale (`12`).
9. **Given** `EACH_QUORUM` (or any level that waits on an asynchronous follower) and a container that did not opt in, **When** the query is planned, **Then** it is refused naming the level and the asynchronous group (`04` FR-074).

---

### User Story 3 - Transactions with a closed isolation set (Priority: P2)

A SQL client starts a transaction, issues several statements, and commits or rolls back. The planner attaches an isolation level from the closed set: `READ COMMITTED` (SQL default) and `SNAPSHOT` where the type supports it. **`SERIALIZABLE` is refused** as a product non-goal (`15`/`16`). Cassandra-style consistency (`ONE` / `LOCAL_ONE` / `TWO` / `QUORUM` / `EACH_QUORUM`) is **not** SQL isolation; this stack attaches both, and does not treat one as the other. Redis and similar single-command protocols remain auto-commit unless they use a documented multi-command transaction mapping. Distributed transactions from L1 (`04` FR-076/FR-077) are used when the plan spans shards or nodes that require atomic commit. First-binary PostgreSQL has **no `BEGIN` and no `COPY`** (`16`); those verbs still belong to this stack for the complete product (`15`). Slice 8 (`16`) is when distributed transactions and query-beyond-CRUD land in an implementation milestone.

**Why this priority**: Isolation "if needed" is now a closed set (`15`); without it, SQL clients cannot reason about concurrency. Follows Stories 1–2. Not required for the first binary.

**Independent Test**: Single-node then two-shard: BEGIN/INSERT/COMMIT, ROLLBACK, concurrent `READ COMMITTED` readers, and a distributed commit with one participant stopped mid-commit (`04` resolution rule).

**Acceptance Scenarios**:

1. **Given** a complete-product PostgreSQL session, **When** it `BEGIN`s, inserts a row, and `COMMIT`s, **Then** a later session sees the row; **When** it `ROLLBACK`s instead, **Then** the row is absent.
2. **Given** no isolation specified on a SQL transaction, **When** it runs, **Then** applied isolation is `READ COMMITTED`.
3. **Given** `SNAPSHOT` requested on a type that supports it, **When** the transaction runs, **Then** statements in that transaction see a consistent snapshot as documented for that type.
4. **Given** `SERIALIZABLE` requested, **When** the query is planned, **Then** it is refused as a product non-goal, naming that the allowed set is `READ COMMITTED` and `SNAPSHOT`.
5. **Given** a planned query that spans shards and declared a distributed transaction, **When** one participant becomes unreachable mid-commit, **Then** every participant ends in the same durable outcome and none stay undecided past the documented bound (`04` FR-077).
6. **Given** a deadlock or abort, **When** it occurs, **Then** the client receives a retryable transaction error in the protocol's form and no partial commit is visible.
7. **Given** a SQL session that also carries quorum `QUORUM`, **When** a transaction runs, **Then** isolation remains `READ COMMITTED` (unless `SNAPSHOT` was requested) and quorum remains `QUORUM`; neither is rewritten into the other.

---

### User Story 4 - Concurrent planning, admission, prepared statements, and COPY (Priority: P2)

The planner may attach a concurrency level for parallel execution of a query. Admission limits from `15` (max concurrent queries per node and per namespace, max query memory) are enforced: overflow **rejects** with a named error, or spills to disk where the plan documents spill. Prepared statements execute through this stack on protocols whose MUST subset includes them — including first-binary PostgreSQL extended/prepared (`16`). COPY inbound and outbound on PostgreSQL execute through this stack in the complete product (`15`); first-binary PostgreSQL returns not-supported for COPY.

**Why this priority**: Prevents unbounded memory and makes the `15` limits real. Prepared statements are in the first binary; COPY is complete-product compatibility.

**Independent Test**: Drive concurrent queries past the documented max (expect reject); run a prepared `SELECT`; on a complete-product build, COPY in and out a small table.

**Acceptance Scenarios**:

1. **Given** a documented max concurrent queries N on a node, **When** N+1 queries are in flight, **Then** the extra query is rejected with an error naming the limit.
2. **Given** a query whose working set exceeds the documented memory cap, **When** the plan allows spill, **Then** it completes using spill; **When** it does not, **Then** it is rejected naming the cap.
3. **Given** a prepared statement in PostgreSQL (first binary or complete product) or CQL (complete product), **When** it is executed with bound values, **Then** results match the equivalent ad-hoc statement.
4. **Given** COPY inbound and outbound on complete-product PostgreSQL for a `Relational Table`, **When** a file of rows is loaded and then copied out, **Then** logical rows match.
5. **Given** first-binary PostgreSQL, **When** COPY is issued, **Then** not-supported is returned and nothing is stored (`16`).

---

### User Story 5 - MapReduce, shuffle, aggregation, join, and subscribe (Priority: P3)

An analyst runs a heavy aggregation or join that the planner implements with MapReduce, shuffle, and the aggregation/join engines. Data exchange between nodes uses the `internode` handler (`12`), not a client protocol port. Replica and shuffle-worker choice among candidates that already satisfy quorum ranks by the topology ladder (near before far), then measured RTT and HLC-skew health. The client may start the work as a job, receive a job id, and wait later via a documented SQL/CQL poll (not `LISTEN`/`NOTIFY`), Redis command, HTTP path, or admin. This story is `16` slice 8 (query beyond CRUD); it is required for the complete product, not the first binary.

**Why this priority**: Named in the intent as the distributed processing story; not required for the first shippable binary (`16` slices 1–5) but required for this feature's complete acceptance.

**Independent Test**: Two-node cluster, join two containers, aggregate, subscribe to completion; kill a shuffle peer and observe a named failure or retry per documented policy.

**Acceptance Scenarios**:

1. **Given** two containers that the type catalog allows to be joined, **When** a SQL join is executed, **Then** the result matches the documented join semantics and the plan names the join engine.
2. **Given** a GROUP BY aggregation, **When** it runs across shards, **Then** shuffle uses `internode` and the aggregated result is complete relative to reachable shards, or fails per Story 2 if a required shard is unavailable.
3. **Given** a long job, **When** the client starts it as a job and waits via the documented SQL/CQL poll (not `LISTEN`/`NOTIFY`), Redis command, HTTP path, or admin, **Then** completion (or documented partial chunks) is delivered without the original request staying open, and cancel of that wait stops the job.
4. **Given** a MapReduce job whose shuffle peer fails, **When** the failure is detected, **Then** the job either retries within the timeout or fails naming the failed stage; it does not hang.
5. **Given** several candidate shuffle workers that already satisfy quorum, **When** the planner places tasks, **Then** it prefers the nearer ladder rung, then lower measured RTT once samples exist, and treats excessive HLC skew as a health signal not to prefer that peer (`04`/`12`).
6. **Given** an Elasticsearch aggregation beyond the engines this feature documents (`15` MUST NOT), **When** it is submitted, **Then** the client receives not-supported; the planner MUST NOT silently approximate it.

---

### Edge Cases

- Query text or result larger than the documented max (`15`): reject before planning or while producing results, naming the limit; do not hang.
- Buffer-full on the node (`01`): reject the new query; do not stall the worker pool.
- Mixed protocol in one transaction: a transaction is bound to one session and one protocol; switching protocols is a new session.
- Empty result vs unavailable shard: empty is a successful zero-row result; unavailable is an error (or a named partial only if the partial-results option is on).
- Partial-results option omitted: treated as off; never a silent partial.
- Memory-mode replica counted toward read quorum: allowed per `04`; a following restart may lose unreplicated memory content (`03`/`13`). Memory-mode acknowledgements MUST NOT satisfy write quorum on persistent or hybrid containers.
- Nested L4 composition: planner walks the composition per `03` write/read rules; cycles never reach execution. Cross-member atomic writes are L1 distributed transactions, not a composition feature (`03`).
- EXPLAIN on a MUST NOT verb: not-supported, same as executing it.
- Coordinator in a follower domain with `LOCAL_QUORUM` and no local replica: reject if explicit; clamp if the level came from a default (`02`).
- HLC samples from two `quorum_domain`s: MUST NOT be used to rank or to resolve conflicts; ranking uses ladder + RTT; conflict across domains follows the source log (`12`).
- Query timeout, shuffle retry budget, and related bounds: configurable per cluster (and per destination group where applicable); none MAY be hardcoded on the assumption of a short round trip (`04` FR-073).
- `LISTEN`/`NOTIFY` used to wait for a job: not-supported (`15`); the documented job-id poll is the SQL wait path.

## Requirements *(mandatory)*

### Functional Requirements

**Canonical IR and native protocol mapping**

- **FR-001**: This feature MUST own a single canonical execution representation (IR). Every protocol handler (`02`) MUST submit work as that IR or as a parse tree this feature translates into that IR. A protocol MUST NOT execute queries with a private engine.
- **FR-002**: PostgreSQL-shaped SQL (PostgreSQL handler) and ClickHouse SQL (`clickhouse` and `clickhouse-http`) MUST parse into that IR. CQL, Redis commands, Elasticsearch query DSL (MUST subset), S3, and WebDAV verbs MUST map into that IR (`15`). First-binary handlers are PostgreSQL and Redis only (`16`); omitted handlers remain owed for the complete product.
- **FR-003**: Semantically equivalent operations through different protocols over the same containers MUST yield the same logical results (already required by `02`; this feature is the place they are computed).
- **FR-004**: Verbs in a protocol's MUST NOT set MUST NOT be planned; the handler's not-supported error is the only response. First-binary PostgreSQL COPY and BEGIN MUST be treated as not-supported (`16`) even though this stack owns them for the complete-product MUST subset (`15`).

**Planner, scheduler, executor**

- **FR-005**: The stack MUST include a query parser, query planner, task scheduler, and task executor as distinct, inspectable stages on the execution record.
- **FR-006**: The planner MUST match each operation to the target type's operation set (`03`) and refuse unsupported operations before touching data.
- **FR-007**: The planner MUST attach timeout and quorum to every query, using client/session/container/global precedence already defined by `02`/`04`. A quorum that came from a default and exceeds replica count MUST be clamped and reported; a quorum the client set explicitly MUST be rejected if unsatisfiable. `LOCAL_*` MUST mean the coordinator's `quorum_domain`, not a ladder key (`04`/`12`).
- **FR-008**: The planner MUST implement concurrency level when specified and when the plan can use it; the default concurrency MUST be documented.
- **FR-009**: Execution state for an in-flight query MUST be queryable through admin surfaces (`01`): identity, stage, applied timeout/quorum/isolation/concurrency, elapsed time, coordinating node, replicas contacted.

**Fault tolerance, cancel, subscribe, domain-aware execution**

- **FR-010**: If all replicas holding a required part of the data are down, the executor MUST tell the user that this part is not available now, naming the part. It MUST NOT return a silent empty success. A query that spans several shards or partitions MUST be fanned out; quorum MUST be applied per shard; a shard that fails to meet it MUST fail the query naming the shard and the level, unless the partial-results option is on (`04` FR-053, FR-011).
- **FR-011**: Partial results MUST be returned only when the **partial-results option** is on. That option uses the same precedence family as timeout and quorum (`02`): per query, then session, then a protocol-native field where one exists. The default is **off**. When it is off, a missing required part fails the query as a whole. When it is on, reachable parts are returned and unavailable parts are named; the client MUST NOT be told the result is complete.
- **FR-012**: Timeout expiry MUST produce the protocol's timeout error within one second of the deadline and MUST stop resource use within that window. Timeout values MUST be configurable and MUST NOT be fixed on the assumption of a low round trip (`04` FR-073).
- **FR-013**: Client disconnect and explicit cancel MUST stop the query within the same window and record the outcome as cancelled.
- **FR-014**: The database MUST offer a subscribe-on-results interface so a client can wait asynchronously for completion (or documented chunks) without holding the original request. Starting such a job MUST return a **job id** shared with admin (`01`). The client MUST be able to poll or wait with a documented SQL/CQL query (not `LISTEN`/`NOTIFY`, which remain MUST NOT in `15`), a documented Redis command, or a documented HTTP path. Cancel of that wait MUST stop the job. Subscribe is complete-product / slice 8 (`16`); it remains a requirement of this feature.
- **FR-031**: A write received by a coordinator in a log-follower domain MUST be forwarded to the source. `LOCAL_*` write MUST still require the corresponding durable acknowledgement in the source. `LOCAL_*` read on a follower MAY be served from a local applied replica and MAY be stale (`12`). The data path MUST remain leaderless in the source domain (`04` FR-045): this feature MUST NOT introduce a mandatory write serializer for ordinary replicated containers.
- **FR-032**: `EACH_QUORUM` and any other level that requires an acknowledgement from an asynchronous follower MUST be refused unless the container declared waiting to honour that level (`04` FR-074). A queued remote send MUST NOT count as an acknowledgement.
- **FR-033**: For persistent and hybrid containers, an acknowledgement that counts toward write quorum MUST wait for the durability contract (`13`), not for an in-memory write alone. Memory-mode containers count memory acknowledgements as defined by `04`.

**Transactions and isolation**

- **FR-015**: Isolation levels the planner MAY attach are only `READ COMMITTED` and `SNAPSHOT` (where the type supports it). SQL default is `READ COMMITTED`. `SERIALIZABLE` MUST be refused as a product non-goal (`15`/`16`).
- **FR-016**: `SNAPSHOT` MUST be refused on types that do not support it, naming the type.
- **FR-017**: Replica and shuffle-worker choice among candidates that already satisfy the requested quorum MUST rank by the cluster topology ladder: the first ladder key that differs, near before far. Measured internode RTT, and HLC skew as a health signal, MUST override that rank once samples exist (`04` FR-001b, `12` FR-018). A missing ladder value is not local. HLC MUST NOT be compared across `quorum_domain`s. `LOCAL_*` is the coordinator's `quorum_domain`, not “same AZ”.
- **FR-018**: BEGIN/COMMIT/ROLLBACK on SQL protocols in the complete-product MUST subset MUST be honored. ROLLBACK MUST make none of the transaction's writes visible. First-binary PostgreSQL MUST NOT require BEGIN (`16`).
- **FR-019**: When the plan requires L1 distributed transactions, the executor MUST use that capability (`04` FR-076); commit MUST resolve to a single durable outcome for every participant; a participant unreachable mid-commit MUST NOT stay undecided past the documented bound (`04` FR-077). This is slice 8 (`16`), still owed by this feature for the complete product.
- **FR-020**: Deadlock or abort MUST surface as a retryable transaction error in the protocol's form with no partial commit visible.
- **FR-034**: Cassandra-style consistency MUST NOT be treated as SQL isolation. The planner MAY attach both to one query; it MUST NOT rewrite one into the other (`15` FR-007).

**Admission, prepared statements, COPY, EXPLAIN**

- **FR-021**: Documented max concurrent queries per node and per namespace (`15`/`07`) MUST be enforced by rejecting the extra query with a named error.
- **FR-022**: Documented max query memory MUST be enforced by spill-to-disk if the plan allows it, otherwise by reject with a named error.
- **FR-023**: Prepared statements on protocols whose MUST subset includes them MUST execute through this stack with bound values. First-binary PostgreSQL extended/prepared is in that subset (`16`).
- **FR-024**: COPY inbound and outbound on complete-product PostgreSQL MUST execute through this stack for types that accept tabular data. First-binary PostgreSQL MUST return not-supported for COPY (`16`).
- **FR-025**: EXPLAIN on SQL protocols MUST return at least a logical plan without executing the query. First binary MAY still offer EXPLAIN (`15`).

**MapReduce and engines**

- **FR-026**: The stack MUST include MapReduce, shuffle, aggregation, join, and data-exchange engines, invoked when the planner selects them. These engines are slice 8 (`16`) for implementation milestones and remain complete-product requirements of this feature.
- **FR-027**: Shuffle and data exchange between nodes MUST use the `internode` handler (`12`), not a client protocol port and not an `admin`/`admin-http` port.
- **FR-028**: Join and aggregation semantics MUST be documented per operation (inner/outer as applicable to SQL MUST subset; grouping keys).
- **FR-029**: A failed MapReduce/shuffle stage MUST retry within the query timeout or fail naming the stage; it MUST NOT hang.
- **FR-035**: Elasticsearch aggregations (and any protocol verb) beyond the engines and operation sets this feature documents MUST be refused as not-supported (`15`); they MUST NOT be silently approximated.

**Observability touchpoint**

- **FR-030**: Query-processing figures required by `08` FR-006 MUST be produced by this feature: kind of query, totals, errors, in-flight, rows/documents/values returned, hits/misses, lookup probes, index vs full scans, retries, histograms of execution/queue/wait and of result size, with labels namespace, schema, datatype, node if applicable, `storage_type`, drive or memory name, and error type when applicable. Series names stay in `08`; this feature guarantees the counts exist.

### Key Entities

- **Canonical IR**: The single execution representation all protocols compile into. Attributes: operations, container references, projections, predicates, joins, aggregations, declared isolation, concurrency, timeout, quorum, partial-results option.
- **Logical Plan / Physical Plan**: Planner output. Logical plan is what EXPLAIN shows at minimum. Physical plan names engines (scan, join, aggregate, MapReduce) and placement of tasks.
- **Execution Record**: Per-query inspectable state: identity, protocol, principal, namespace, applied options (including clamp provenance), stage, start time, outcome, coordinating node, replicas contacted, durable vs non-durable acknowledgements.
- **Transaction**: Session-scoped unit of work with isolation from the closed set, optional distributed-transaction flag, commit or abort outcome. Distinct from Cassandra-style consistency.
- **Subscription / Job**: Identified by a job id. A client waits via documented SQL/CQL poll (not `LISTEN`/`NOTIFY`), Redis command, HTTP path, or admin; cancellable. Same object on every surface.
- **Task**: Scheduled unit on a node (map, reduce, scan, shuffle send/receive).
- **Unavailability Error**: Named data part with zero live replicas; distinct from an empty successful result.
- **Source / log-follower coordination**: Execution-time routing of writes to the container's source `quorum_domain` and of `LOCAL_*` reads that MAY be stale on a follower (`12`).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of MUST-subset smoke workflows (`15`) for implemented protocol handlers complete through the shared planner, with 0 protocol-private engines in the execution record. First-binary conformance uses the `16` PostgreSQL and Redis subsets; complete-product conformance uses the `15` matrix for implemented handlers.
- **SC-002**: Semantically equivalent reads through two protocols over the same dataset agree on logical content in 100% of conformance cases.
- **SC-003**: 100% of queries in the conformance suite have an inspectable timeout and quorum; 99% of timeout tests return a protocol timeout error within one second of the deadline and release resources in that window.
- **SC-004**: 100% of disconnect/cancel tests stop the query within the timeout window and record cancelled.
- **SC-005**: 100% of queries that need a shard with zero live replicas and the partial-results option off return a named unavailability error and fail as a whole; 100% with the option on return reachable data plus the named missing part; 0 silent empty successes; 0 complete-success flags on partials.
- **SC-006**: 100% of SQL BEGIN/COMMIT and BEGIN/ROLLBACK cases in the suite (complete-product MUST subset) match the visibility rules; 100% of `SERIALIZABLE` requests are refused as a non-goal.
- **SC-007**: 100% of admission-limit tests reject the overflowing query with the limit named; 0 hangs.
- **SC-008**: EXPLAIN on 100% of sample SQL queries in the suite (where EXPLAIN is in the applicable MUST subset) returns a logical plan without executing them.
- **SC-009**: A documented join and a documented aggregation across two nodes complete with correct results in 100% of conformance runs when all required shards are available (complete product / slice 8).
- **SC-010**: Subscribe-on-results delivers completion for 100% of sample long jobs without requiring the original request to stay open; 100% of those waits succeed through the documented SQL (or Redis/HTTP) poll path as well as admin; cancel of the wait stops the job (complete product / slice 8). 0 uses of `LISTEN`/`NOTIFY` as the wait path.
- **SC-011**: 100% of writes submitted to a coordinator in a log-follower domain are forwarded to the source; 100% of `LOCAL_ONE` writes on a follower require a durable source acknowledgement; 100% of default unsatisfiable quorums are clamped and 100% of explicit unsatisfiable quorums are rejected before execution.
- **SC-012**: 100% of first-binary PostgreSQL COPY and BEGIN attempts return not-supported and store nothing; 100% of first-binary PostgreSQL auto-commit DML/DDL and prepared statements in the `16` smoke complete through this stack.

## Assumptions

- Protocol verb matrices, wire versions, size limits, and isolation set are owned by intent `15`; this feature consumes them.
- Quorum meaning, replica sets, clamp/reject, fan-out, `EACH_QUORUM` opt-in, and distributed-commit protocol are owned by `04`/`02`; this feature attaches levels and invokes them.
- Type operation sets and composition walk are owned by `03`.
- Internode transport, `quorum_domain`, source vs log-follower, HLC-in-domain, and RTT/skew publication are owned by `12`; this feature is a client of `internode` for shuffle/data-exchange and of those measurements for ranking.
- Durability of an acknowledgement is owned by `13`; this feature waits for it on persistent/hybrid write quorum.
- Parser front-ends may live next to protocol handlers (`02`) but MUST produce this feature's IR; they are not a second executor.
- Default timeout remains the order of tens of seconds (`02`). Default SQL isolation is `READ COMMITTED`. `SERIALIZABLE` is a non-goal, not a later milestone. Partial-results option default is off; syntax lives with the `02` option surfaces (SQL `SET`/hint, Redis `SS.OPTIONS`, HTTP header, CQL payload) plus any protocol-native field.
- Default query concurrency is sequential (one execution stream) unless the client or plan specifies parallelism the planner can use.
- Subscribe-on-results is a job id plus poll/wait: documented SQL/CQL queries (not `LISTEN`/`NOTIFY`), a documented Redis command, a documented HTTP path, and admin (`01`). All of those surfaces MUST name the same job.
- MapReduce, joins, aggregation, subscribe, and distributed transactions are not required for the first shippable binary (`16` slices 1–5 / slice 8) but are required for this feature's complete acceptance (Story 5 and Story 3 distributed-commit scenarios).
- Prepared statements execute through this stack in the first binary for PostgreSQL; COPY and BEGIN execute through this stack when the applicable MUST subset includes them (`15` complete product).
- Metric series names are `08`; this feature guarantees the counts exist.
- Index-rebuild jobs that this stack enqueues are listed in `08` FR-012 with behaviour owned jointly with `03`.
- No [NEEDS CLARIFICATION]: isolation set, IR ownership, cancel, unavailability, ladder+RTT+skew ranking, write-forward, first-binary cut, `EACH_QUORUM` default-refuse, subscribe wait path (job id + poll), and partial-results option (same family as timeout/quorum, default off) are closed.

## Out of Scope

- **Listen ports and handshake** (intent `02`).
- **Type inventory and catalog** (intent `03`) except as planning input.
- **Placement, repair, and quorum arithmetic** (intent `04`) except invoking them.
- **Admin visualization of shards** (intent `09`).
- **MUST/MUST NOT verb lists** (intent `15`) except enforcing them when planning.
- **First-binary verb cut** (intent `16`) except consuming which verbs this stack must execute in a given milestone.
- **Internode framing, HLC, LWW, membership** (intents `12`/`11`) except using `internode` for shuffle, RTT/skew for ranking, and source/follower routing at execution time.
- **AuthZ of who may run a query** (intent `14`); the executor assumes the handler already authorized the session.
- **A second query engine per protocol** — product non-goal (`16`), not later.
- **SQL `SERIALIZABLE`** — product non-goal (`15`/`16`), not later.
- **Drop-in replacement for every feature of the emulated systems** (`15` MUST NOT column, `16`).
- **Per-tenant CPU hard isolation** (`15`/`16`).
- **Kafka as a stored log product**; Log Stream is the type (`16`/`09`/`08`).
- **Byzantine / adversarial node model** (`16`/`12`).
- **`multi_active=on` create in the first binary** (`16`/`12`).
- **A SpaceStorage-native client protocol** (`16`).

# Research: Query Execution, MapReduce, Transactions, and Fault-Tolerant Results

**Feature**: `005-query-execution` | **Date**: 2026-09-16

Each item resolves a Technical Context unknown or a technology choice. Format: Decision / Rationale / Alternatives considered. Inputs: [spec.md](spec.md) (incl. clarify 2026-09-16 B/A), constitution 1.3.0, intent `05`, sibling plans `002` (IR/`QueryEngine`/`LocalEngine`), `004` (coordinator, quorum, 2PC, ranking), `012` (internode, `quorum_domain`, RTT/HLC), `015` (isolation, admission, dialects), `016` (first binary vs slice 8).

## R1. Canonical IR is `LogicalRequest`, not a new AST

- **Decision**: The `002` `LogicalRequest` enum **is** the canonical IR this feature owns (FR-001). Additive variants/fields: `Explain { inner }`, `CopyIn`/`CopyOut`, `Txn { Begin, Commit, Rollback }`, `SubscribeWait`/`Cancel { exec_id }`, plus `QueryOptions` fields `isolation`, `concurrency`, `partial_ok` (Sourced, default off), `async_job`. Protocol parsers stay in handler crates and continue to lower into this enum.
- **Rationale**: `002` already forbids protocol-private engines and defined the boundary `05` would fill. A second IR would be a second engine in disguise (constitution IV / `016` non-goal).
- **Alternatives considered**: Calcite/DataFusion `LogicalPlan` (rejected: Arrow type system + C-adjacent parquet stacks; Principle I/IV); moving `sqlparser` into `exec` (rejected: dialects belong at the protocol edge).

## R2. One crate: grow `spacestorage-exec`

- **Decision**: `PlannerEngine` lives in `crates/exec`. Trait signatures stay; additive methods (`explain`, `cancel`, `subscribe`) have default bodies so `002` compiles. `LocalEngine` remains under `exec::local` for RF=1 tests. Node wiring selects `PlannerEngine` whenever internodes or RF>1 exist, and also as default in first-binary single-node (so execution records name the shared planner).
- **Rationale**: Handlers already depend on `spacestorage-exec`. A new `crates/query` would split the IR from the engine.
- **Alternatives considered**: New crate (rejected: extra graph edge, duplicate types); delete `LocalEngine` (rejected: `002` unit tests and no-internodes RF=1).

## R3. First binary vs slice 8 feature flag

- **Decision**: Cargo feature `query-distributed` (enabled by `release-profile` complete-product / slice 8) compiles join, aggregation beyond simple COUNT on one shard, MapReduce, shuffle, subscribe, SQL `BEGIN` execution, COPY execution. Default/first-binary still plans and executes `Ddl`/`Point`/`Scan`/`Mutate`/`TypeOp`/`ObjectReq`/`Batch` (non-atomic) and prepared statements. COPY/BEGIN are **handler-refused** in the first-binary dialect even if the engine could run them.
- **Rationale**: `016` R7 and slice table: “query beyond CRUD” is slice 8; first-binary PG smoke is auto-commit CRUD. Implementing BEGIN in the engine while the dialect forbids it is fine; the dialect is the gate.
- **Alternatives considered**: Two `QueryEngine` impls selected at runtime only (rejected: unused shuffle code in the first binary); wait to replace `LocalEngine` until slice 8 (rejected: SC-001 requires the shared planner on first-binary smokes).

## R4. Stages on the execution record

- **Decision**: `ExecutionStage = Received | Bound | Planned | Scheduled | Running { task } | Finalizing | Done | Failed | Cancelled`. FR-005 “parser, planner, scheduler, executor” map to Bound / Planned / Scheduled / Running. EXPLAIN stops after Planned and never enters Running.
- **Rationale**: Admin `executions` already exists (`002`). Stages make FR-009 inspectable without a debugger.
- **Alternatives considered**: OpenTelemetry-only spans (rejected: spec requires queryable execution state on admin surfaces).

## R5. Planning algorithm (no cost-based optimizer in v1)

- **Decision**: Rule-based planner: (1) reject MUST NOT / type-unsupported / SERIALIZABLE / unsatisfiable explicit quorum / EACH_QUORUM without opt-in; (2) walk L4 composition per `003`; (3) attach timeout/quorum/isolation/concurrency; (4) choose engines: point/scan/mutate locally or via `004` coordinator fan-out; join → hash or nested-loop (inner/left only); aggregate → local then shuffle reduce; multi-shard scan → fan-out + merge. No statistics-driven join reorder in this feature.
- **Rationale**: Spec requires matching type operations and naming engines on EXPLAIN, not a research optimizer. Cost-based planning can expand later without changing IR.
- **Alternatives considered**: DataFusion optimizer (rejected: R1); always nested-loop (rejected: FR-026 MapReduce/shuffle exist for a reason, but only under `query-distributed`).

## R6. Replica and shuffle ranking

- **Decision**: Implement `rank.rs` as a pure function over `004` topology ladder + `012` published RTT and in-domain HLC skew. Order: candidates that already satisfy quorum → sort by first differing ladder key (near first) → stable-sort by RTT once sample exists → deprioritize peers whose in-domain skew exceeds tolerance. Missing ladder value sorts as farthest. Cross-domain HLC is ignored.
- **Rationale**: Spec FR-017; `004` FR-001b; `012` FR-018. Ranking is planning, not a new placement planner.
- **Alternatives considered**: Random replica (rejected: locality MUSTs); using `az` as `LOCAL_*` (rejected: `LOCAL_*` is `quorum_domain`).

## R7. Coordination: consume `004`, do not duplicate TCP

- **Decision**: Multi-shard fan-out, durable-ack filter, follower write-forward, `EACH_QUORUM` policy, and 2PC are calls into `placement::Coordinator` / `placement::txn`. `PlannerEngine` builds a `PhysicalPlan` of tasks; the coordinator executes replica contact. Unavailability errors (`part_unavailable{shard,partition,replicas}`) are produced here from coordinator failures so the protocol renderer can shape them.
- **Rationale**: `004` already specified internodes fan-out and FR-053 per-shard quorum. A second RPC from `exec` would violate “shuffle uses internodes” and Principle III-ish duplication.
- **Alternatives considered**: `exec` opening internodes sessions itself (rejected: two clients of the same fabric); drivers contacting replicas (rejected: private engine).

## R8. Timeout, cancel, disconnect

- **Decision**: Each execute() wraps work in `tokio::select!` on (a) `opts.timeout` deadline, (b) session cancel token from `protocol-core` (disconnect and client cancel map to the same token), (c) work completion. On (a)/(b): abort tasks, drop spill, emit `Error(Timeout)` or `Error(Cancelled)` within deadline+1s (target +50 ms). In-flight internodes RPCs are cancelled via the existing request-id abort (`012` backpressure path).
- **Rationale**: FR-012/FR-013; `002` already promised timeout around `LocalEngine`.
- **Alternatives considered**: Thread-kill (rejected: Tokio); ignoring disconnect until timeout (rejected: spec).

## R9. Admission defaults

- **Decision**: `query { max_concurrent_per_node 512; max_concurrent_per_namespace 128; max_memory 256MiB; spill off; default_concurrency 1; }`. Overflow without spill → `admission_rejected{limit, current}`. `spill on` allows sort/hash/agg to use `{data_dir}/spill/<id>/` up to `max_memory` of RAM plus spill disk; spill disk full → same named reject. Reload live; in-flight queries keep the snapshot they started with.
- **Rationale**: `015` requires documented limits and hard reject; no numbers were specified, so these are the documented defaults (research choice). 512 concurrent is well below typical `worker_threads` × thousands of tasks; 256 MiB per query bounds a noisy neighbor before `07` quotas land.
- **Alternatives considered**: Unlimited until `07` (rejected: FR-021/022); spill on by default (rejected: surprising disk fill on first binary).

## R10. Isolation and SQL transactions

- **Decision**: `IsolationLevel { ReadCommitted, Snapshot }`. Default SQL = `ReadCommitted`. `SET TRANSACTION ISOLATION LEVEL SERIALIZABLE` / `BEGIN ISOLATION LEVEL SERIALIZABLE` → `NotSupported { what: "SERIALIZABLE", allowed: "READ COMMITTED, SNAPSHOT" }` before plan. `SNAPSHOT` on a type whose descriptor `isolation.snapshot != true` → `snapshot_unsupported{type}`. Session txn state in `exec::txn`: writes buffer per container until Commit; Rollback drops the buffer; visibility per Read Committed (each statement sees latest committed) or Snapshot (statement-start snapshot token from `003`/`013` MVCC if the type supports it). First-binary dialect never enters this state machine. Multi-shard Commit calls `004` 2PC when any participant set size > 1 or `capability.distributed_transactions` is on.
- **Rationale**: FR-015–FR-020, FR-034; `004` contract already says SQL BEGIN/isolation are `05`.
- **Alternatives considered**: Open isolation set (rejected: grill / `016` non-goal); implementing SERIALIZABLE via SSI (rejected: product non-goal).

## R11. Consistency is not isolation

- **Decision**: `QueryOptions.quorum` and `QueryOptions.isolation` are independent fields. Renderers MUST show both. No code path may set isolation from a CQL consistency level or vice versa.
- **Rationale**: `015` FR-007; spec FR-034.
- **Alternatives considered**: Mapping `QUORUM` → `SNAPSHOT` (rejected: different meaning).

## R12. Prepared statements and COPY

- **Decision**: Prepared: handler stores the lowered `LogicalRequest` with param slots; EXECUTE fills `CanonicalValue`s and calls `QueryEngine` (already how `002` extended query should work — this feature makes Bound a first-class stage and guarantees bound execution matches ad-hoc). COPY: `CopyIn` streams rows as `Mutate::Insert` batches; `CopyOut` is a `Scan` with text/binary encoding in the PG handler. Complete-product dialect only.
- **Rationale**: FR-023/FR-024; `pgwire` already frames COPY (`002` R2).
- **Alternatives considered**: Server-side SQL re-parse on every EXECUTE (rejected: would miss bind-stage inspectability).

## R13. Join, aggregation, MapReduce, shuffle

- **Decision**: Join kinds: `Inner`, `Left` (SQL MUST subset). Implementation: in-memory hash join if build side fits `max_memory`; else spill hash; else nested-loop if both sides are point-indexed. Aggregation: partial agg per shard, shuffle by grouping key via internodes `ShufflePush`/`ShufflePull`, final agg. MapReduce: planner emits map tasks (per shard) and reduce tasks (by key hash); shuffle uses internodes only. Failed stage retries until query timeout, then `Error(StageFailed { stage })`. Elasticsearch bucket aggs beyond `terms` + metric min/max/sum/avg/count → `NotSupported` (FR-035); no silent approximation.
- **Rationale**: FR-026–FR-029, FR-035; `002` already lowered 2-way join and simple aggs.
- **Alternatives considered**: Always pull to coordinator (rejected: does not scale and ignores shuffle MUST); implementing full ES agg tree (rejected: `015` MUST NOT).

## R14. Subscribe-on-results

- **Decision**: `QueryOptions.async_job = true` (or `SET spacestorage.async = on` then the statement) returns `ExecutionEvent::Accepted { exec_id }` immediately and runs in the background. **One job object** is waitable on all of: documented SQL/CQL (`spacestorage.job_wait` / `system.spacestorage_jobs` — **not** `LISTEN`/`NOTIFY`), Redis `SS.JOB *`, HTTP `/_spacestorage/jobs/{id}`, admin `GET /v1/jobs/{id}`, CLI `spacestorage jobs wait`, internodes `SubscribeNotify` for peers. Cancel of any wait cancels the job.
- **Rationale**: Clarify 2026-09-16 option B. Stock clients must wait without becoming operators and without `LISTEN`/`NOTIFY` (`015` MUST NOT). Admin remains; it is not the only path.
- **Alternatives considered**: Admin/CLI only (rejected: stock SQL/Redis clients could not finish Story 5); PG `LISTEN`/`NOTIFY` (rejected: `015`); requiring the original TCP connection (rejected: spec).

## R15. EXPLAIN

- **Decision**: `EXPLAIN` / `EXPLAIN SELECT` returns a text (and JSON via admin) logical plan: containers, operations, engines, attached timeout/quorum/isolation/concurrency, shard fan-out. No execution, no locks. `EXPLAIN` of MUST NOT verbs → not-supported. First binary MAY implement this (handler allowed); complete product MUST.
- **Rationale**: FR-025; `015` FR-008.
- **Alternatives considered**: EXPLAIN ANALYZE in this feature (deferred: needs runtime counters already in `08`; not required).

## R16. Metrics

- **Decision**: `exec` increments the `08` FR-006 families via `node::stats` hooks already reserved: query totals/errors/in-flight, histograms execution/queue/wait, result bytes, retries, rows returned, hits/misses, scan kind. Labels: `kind`, `namespace`, `schema`, `datatype`, `node`, `storage_type`, `error_type`. Series **names** are defined in `008`; this crate must not invent parallel names.
- **Rationale**: Spec FR-030; constitution observability contract.
- **Alternatives considered**: Private `exec_*` names (rejected: would rename/drop required `08` labels later).

## R17. Timeouts must not assume a short RTT

- **Decision**: Query timeout default remains `30s` (`002` `query_defaults.timeout`). Shuffle retry uses remaining query budget, not a hardcoded 100 ms. Internodes RPC timeout is min(remaining query timeout, `internode.request_timeout` from `012`/`004` FR-073). No constant in `exec` assumes sub-10 ms RTT.
- **Rationale**: FR-012; `004` FR-073.
- **Alternatives considered**: Per-RPC 50 ms default (rejected: planetary examples).

## R18. Conformance split

- **Decision**: First-binary tests always use `PlannerEngine` (SC-001, SC-012). Multi-node unavailability, forward, LOCAL_ONE need the `004` three-node harness (SC-005, SC-011). Join/agg/subscribe tests live under `--features query-distributed` (SC-009, SC-010). SERIALIZABLE refuse can run on one node (SC-006).
- **Rationale**: Matches `016` conformance profiles and this spec’s story priorities.
- **Alternatives considered**: One monolithic suite that fails first-binary CI on missing joins (rejected: slice 8).

## R19. Partial-results option

- **Decision**: `QueryOptions.partial_ok: Sourced<bool>`, default **false**. Same precedence family as timeout/quorum (`002`): per query, then session, then protocol-native field if any, then built-in default off. Syntax: SQL `SET spacestorage.partial = on` / hint `/*+ spacestorage: partial=on */`; Redis `SS.OPTIONS SET partial on` / `SS.WITH partial on`; HTTP `X-SpaceStorage-Partial: on`; CQL custom payload `spacestorage.partial`. When off, a missing required shard fails the whole query. When on, reachable shards return plus named `PartUnavailable`; `Done.partial = true` — never advertised as complete.
- **Rationale**: Clarify 2026-09-16 option A. Reuses `002` option plumbing; default off preserves “no silent partial”.
- **Alternatives considered**: Admin-only opt-in (rejected: protocol clients could not request it); container-only setting (rejected: no per-query override).

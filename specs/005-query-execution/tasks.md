---
description: "Task list for query execution, MapReduce, transactions, and fault-tolerant results"
---

# Tasks: Query Execution, MapReduce, Transactions, and Fault-Tolerant Results

**Input**: Design documents from `/specs/005-query-execution/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md), [data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: Requested. Plan Testing section + SC-001–SC-012 + Independent Tests in [spec.md](spec.md). Unit tests in `crates/exec`; conformance in `crates/conformance/tests/` (`planner_smoke`, `equivalence`, `timeout_cancel`, `unavailability`, `isolation`, `admission`, `explain`, `forward_local`, and `query-distributed`: `join_aggregate`, `subscribe`). Write failing tests first where listed.

**Organization**: Tasks grouped by user story so each story is independently implementable and testable. First-binary MVP = User Story 1 + User Story 2 + User Story 4 **admission/spill** (not COPY) (+ shared foundation), matching Session 2026-09-16 / [plan.md](plan.md) Summary (timeout, quorum, cancel, unavailability, admission through this stack). US4 is split: admission/spill/concurrency limits = first-binary; COPY in/out = complete-product. Slice 8 / `query-distributed` / complete-product = User Stories 3 (BEGIN/2PC), US4 COPY, and 5 (join/agg/MapReduce/subscribe).

**Scope**: Grow `crates/exec` from the `002` seam into `PlannerEngine`. No new binary, no second IR, no DataFusion/Arrow. Fan-out/2PC/quorum math stay in `004`/`placement`; shuffle framing in `012`/`internode`. First-binary dialect continues to refuse `COPY`/`BEGIN` in the handler before IR.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: [US1]…[US5] on user-story phase tasks only
- Every task includes an exact file path

## Path Conventions

- Core engine: `crates/exec/src/`
- Slice-8 engines: `crates/exec/src/engines/` behind Cargo feature `query-distributed`
- Config / node / admin / CLI / handlers / conformance as in [plan.md](plan.md) Project Structure

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Module skeleton, Cargo feature, docs/examples from fixtures

- [ ] T001 Extend `crates/exec/src/lib.rs` to declare modules `planner`, `schedule`, `engine`, `admission`, `txn`, `cancel`, `rank`, `spill` (and `engines` behind `#[cfg(feature = "query-distributed")]`) while keeping existing `QueryEngine` trait signatures and `local/`
- [ ] T002 Add Cargo feature `query-distributed` in `crates/exec/Cargo.toml` and wire it from `crates/release-profile` complete-product / slice 8 so default/first-binary builds omit join/MapReduce/subscribe/shuffle engines
- [ ] T003 [P] Create empty module stubs `crates/exec/src/{planner,schedule,engine,admission,txn,cancel,rank,spill}.rs` and `crates/exec/src/engines/{mod,join,aggregate,mapreduce,shuffle,subscribe}.rs` (engines gated by `query-distributed`)
- [ ] T004 [P] Create `docs/query-execution.md` outlining stages, closed isolation set (`READ COMMITTED`, `SNAPSHOT`; `SERIALIZABLE` refused), admission defaults, and ladder-then-RTT ranking
- [ ] T005 [P] Copy [contracts/fixtures/query-block.conf](contracts/fixtures/query-block.conf) to `docs/examples/query.conf`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Shared types, options, errors, stages, config, and `PlannerEngine` seam every story needs. No user-story work until this phase completes.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [ ] T006 Extend `IsolationLevel { ReadCommitted, Snapshot }` and `Concurrency { Sequential, Parallel { degree: u16 } }` (default `Sequential` / degree 1) plus `partial_ok: Sourced<bool>` (built-in default **false**) and `async_job: Sourced<bool>` (built-in default **false**) on `QueryOptions` in `crates/exec/src/options.rs` per [data-model.md](data-model.md) §1 and [query-options-additions.md](contracts/query-options-additions.md)
- [ ] T007 [P] Add IR variants `Explain`, `CopyIn`, `CopyOut`, `TxnBegin { isolation }`, `TxnCommit`, `TxnRollback`, `SubscribeWait { exec_id }`, `Cancel { exec_id }` and `CopyFormat { Text, Csv, Binary }` to `LogicalRequest` in `crates/exec/src/request.rs` per [data-model.md](data-model.md) §2 and [ir.md](contracts/ir.md)
- [ ] T008 [P] Extend `ExecError` in `crates/exec/src/error.rs` with `Timeout`, `Cancelled`, `Unavailable(PartUnavailable)`, `NotSupported`, `Admission`, `Isolation` / `snapshot_unsupported`, `QuorumUnsatisfiable`, `RetryableTxn`, `StageFailed` (and related named codes from contracts)
- [ ] T009 Implement `ExecutionStage` (`Received | Bound | Planned | Scheduled | Running { task } | Finalizing | Done | Failed | Cancelled`) and additive `ExecutionRecord` fields (`stage`, `plan_id`, `isolation_applied`, `concurrency_applied`, `coordinator`, `replicas_contacted`, `acks_durable`, `acks_memory`, `elapsed`, `spill_bytes`, `engine = "planner"`) in `crates/exec/src/record.rs` per [data-model.md](data-model.md) §4 and [planner-executor.md](contracts/planner-executor.md)
- [ ] T010 [P] Define `LogicalPlan`, `PhysicalPlan`, `Task`, `EngineKind`, `PlanId`, `TaskId` in `crates/exec/src/planner.rs` (or `plan.rs`) per [data-model.md](data-model.md) §3
- [ ] T011 [P] Define `RankKey { ladder_distance, rtt, skew_unhealthy }` compare order and `PartUnavailable` in `crates/exec/src/rank.rs` / `error.rs` per [data-model.md](data-model.md) §8–§9
- [ ] T012 [P] Define `AdmissionLimits` defaults (`max_concurrent_per_node` **512**, `max_concurrent_per_namespace` **128**, `max_memory` **256MiB**, `spill` **false**) and `AdmissionToken` / `SpillDir` types in `crates/exec/src/admission.rs` and `crates/exec/src/spill.rs` per [data-model.md](data-model.md) §7 and [admission.md](contracts/admission.md)
- [ ] T013 [P] Define `Transaction` / `TxnState` and `Subscription` / `JobState` structs in `crates/exec/src/txn.rs` and always-compiled `crates/exec/src/subscribe.rs` (re-export / thin wrap from `engines/subscribe.rs` when `query-distributed` is on) per [data-model.md](data-model.md) §5–§6 — core subscription types are **not** gated behind the feature
- [ ] T014 Parse `query { max_concurrent_per_node; max_concurrent_per_namespace; max_memory; spill on|off; default_concurrency; }` with validation codes `query_max_concurrent_zero`, `query_max_memory_zero`, `query_concurrency_zero`, `query_spill_unknown` in `crates/config` per [config-directives.md](contracts/config-directives.md); live-reload applies to **new** queries only
- [ ] T015 Implement session cancel token plumbing in `crates/exec/src/cancel.rs` (disconnect + explicit cancel share one token) and timeout `tokio::select!` wrapper skeleton used by all execute paths (SLA: deadline + 1 s) per [research.md](research.md) R8
- [ ] T016 Implement `PlannerEngine` stub in `crates/exec/src/engine.rs` implementing `QueryEngine` (additive `explain` / `cancel` / `subscribe` methods may have default bodies); retain `LocalEngine` under `crates/exec/src/local/` for RF=1 / `002` unit tests only
- [ ] T017 Wire node default engine to `PlannerEngine` in `crates/node` so production/`spacestoraged` execution records name `engine = "planner"` (not a protocol name, not `local`) per [planner-executor.md](contracts/planner-executor.md) SC-001
- [ ] T018 [P] Extend admin DTOs for stage / applied options / jobs / explain in `crates/admin-proto` and list/get surfaces noted in [config-directives.md](contracts/config-directives.md) (`GET /v1/executions`, `POST /v1/explain`, jobs endpoints as stubs returning not-ready until US5)

**Checkpoint**: `cargo check -p spacestorage-exec` and config validate of [contracts/fixtures/invalid/](contracts/fixtures/invalid/) codes compile. `LocalEngine` still selectable in tests. User stories may start.

---

## Phase 3: User Story 1 - Shared planner for native protocol queries (Priority: P1) 🎯 MVP (part 1 of 3)

**Goal**: First-binary PostgreSQL (simple + extended/prepared, auto-commit DML/DDL; no COPY/BEGIN) and Redis MUST-list on `K/V Store` execute through one `PlannerEngine` and `LogicalRequest`. Type-unsupported ops refused before data. Semantically equivalent cross-protocol reads agree. EXPLAIN MAY return a logical plan without executing. (First-binary MVP also requires Phase 4 US2 + Phase 6 US4 admission/spill — see Organization.)

**Independent Test**: One node — Relational Table + K/V Store; PG smoke + Redis MUST list; write via one protocol and read via the other; `spacestorage executions` shows `engine=planner`. No MapReduce/BEGIN/distributed txn required.

### Tests for User Story 1 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T019 [P] [US1] Add `crates/conformance/tests/planner_smoke.rs` covering SC-001/SC-012: PG INSERT/SELECT/UPDATE/DELETE, CREATE/DROP, prepared round-trip through `PlannerEngine`; `BEGIN`/`COPY` → not-supported and store nothing; Redis MUST-list on KV; execution record `engine == "planner"` and shared stage names (`Received`/`Bound`/… — never a private engine; handler parse→IR is Received/Bound ownership per [planner-executor.md](contracts/planner-executor.md))
- [ ] T020 [P] [US1] Add `crates/conformance/tests/equivalence.rs` covering SC-002: value written via PostgreSQL readable via Redis mapping (and reverse) with identical logical content
- [ ] T021 [P] [US1] Add unit tests in `crates/exec/src/planner.rs` for type-catalog refuse (`unsupported_by_type{container, type, op}`) before any mutate and for MUST NOT / first-binary COPY/BEGIN never becoming `LogicalRequest`
- [ ] T022 [P] [US1] Add `crates/conformance/tests/explain.rs` covering SC-008 (MAY on first binary): `EXPLAIN SELECT` returns logical plan naming container + Scan, stage stops at `Planned`, no data touch

### Implementation for User Story 1

- [ ] T023 [US1] Implement rule-based plan stages Received→Bound→Planned→Scheduled→Running→Done in `crates/exec/src/planner.rs` and `crates/exec/src/schedule.rs` for first-binary IR: `Ddl`, `Point`, `Scan`, `Mutate`, `TypeOp`, `Object`, `Batch` (sequential auto-commit) per [planner-executor.md](contracts/planner-executor.md) and [research.md](research.md) R5
- [ ] T024 [US1] Match each operation to `Datatype::operations()` (`003`) and refuse unsupported ops before touching data in `crates/exec/src/planner.rs` (FR-006); walk L4 composition per `003` rules; stale cycle → `Failed{cyclic_composition}` without execution
- [ ] T025 [US1] Attach timeout and quorum to every query using `002`/`004` precedence in `crates/exec/src/planner.rs` / `options.rs` (full clamp/reject behaviour completed in US2; stubs must still record applied options on the execution record)
- [ ] T026 [US1] Execute planned CRUD tasks via `placement::Coordinator` (or local path when RF=1) inside `crates/exec/src/engine.rs` without handlers calling type mutators except through `QueryEngine` ([ir.md](contracts/ir.md))
- [ ] T027 [P] [US1] Implement Bound stage for prepared/extended parameter fill in `crates/exec/src/planner.rs` so EXECUTE matches equivalent ad-hoc (FR-023); handlers keep storing slotted `LogicalRequest` in `crates/handler-postgresql` (complete-product CQL/ClickHouse/ES/S3/WebDAV IR lowering deferred — see T084–T086 / `016` slice 6)
- [ ] T028 [P] [US1] Implement `LogicalRequest::Explain` completing at Planned (no Running) with containers, ops, engines, attached options in `crates/exec/src/planner.rs`; SQL text via `crates/handler-postgresql`; JSON via admin `POST /v1/explain` in `crates/node`
- [ ] T029 [US1] Keep first-binary PostgreSQL dialect refusing `COPY`/`BEGIN` **before** IR in `crates/handler-postgresql` (SC-012) even though this crate owns those variants for complete-product
- [ ] T030 [US1] Ensure Redis MUST-list commands lower to the same IR and execute through `PlannerEngine` in `crates/handler-redis` with no protocol-private engine
- [ ] T031 [US1] Publish inspectable execution records (identity, stage, applied timeout/quorum, coordinator) on existing admin/CLI `executions` in `crates/node` + `crates/spacestorage` (FR-009 subset for US1)
- [ ] T032 [US1] Unit-test stage transitions and `engine=planner` labelling inside `crates/exec` (debug invalid transitions panic; release → `Failed`); assert `ExecutionRecord` uses shared stage names only (`Received`|`Bound`|`Planned`|…) after handler parse→IR (FR-005 ownership: handler owns Received/Bound parse; shared planner stages thereafter — see [planner-executor.md](contracts/planner-executor.md))

**Checkpoint**: `cargo test -p spacestorage-conformance --features first-binary planner_` and equivalence pass. Quickstart §1 works. US1 path green — continue to US2 and US4 admission/spill before declaring first-binary MVP (do **not** stop after US1 alone).

---

## Phase 4: User Story 2 - Timeout, quorum, cancel, unavailability, domain-aware coordination (Priority: P1) 🎯 MVP (part 2 of 3)

**Goal**: Every query carries timeout + quorum (clamp defaults, reject explicit unsatisfiable). Cancel/disconnect and timeout stop work within deadline+1s. Zero live replicas → named unavailability (fail whole unless `partial_ok`). Multi-shard fan-out with per-shard quorum. Follower writes forward to source. `EACH_QUORUM` refused unless opted in. Replica ranking: topology ladder then RTT / in-domain HLC skew. Required for first-binary MVP (Session 2026-09-16).

**Independent Test**: Three-node replicated container: ONE/TWO queries; stop all replicas of one shard; cancel and timeout a long query; optional follower-domain write forward. No MapReduce required.

### Tests for User Story 2 ⚠️

- [ ] T033 [P] [US2] Add `crates/conformance/tests/timeout_cancel.rs` covering SC-003/SC-004: timeout error within 1 s of deadline and resources released; disconnect/explicit cancel → record `Cancelled` in the same window
- [ ] T034 [P] [US2] Add `crates/conformance/tests/unavailability.rs` covering SC-005: partial off → named `part_unavailable` and fail whole (never silent empty); partial on → reachable rows + named missing part with `Done.partial = true` and not reported complete
- [ ] T035 [P] [US2] Add `crates/conformance/tests/forward_local.rs` covering SC-011: follower-domain write forwarded to source; `LOCAL_ONE` write needs durable source ack; default unsatisfiable quorum clamped; explicit unsatisfiable rejected before data
- [ ] T036 [P] [US2] Add unit tests in `crates/exec/src/rank.rs` for `RankKey` order (ladder distance, then RTT `Some` before `None`, skew unhealthy deprioritized; missing ladder = farthest; cross-domain HLC ignored)

### Implementation for User Story 2

- [ ] T037 [US2] Finish quorum attach in `crates/exec/src/planner.rs` / `options.rs`: default-sourced unsatisfiable → clamp + record `applied.quorum.clamped`; explicit unsatisfiable → `QuorumUnsatisfiable` before data; `LOCAL_*` = coordinator `quorum_domain` ([coordination.md](contracts/coordination.md))
- [ ] T038 [US2] Implement timeout and cancel completion in `crates/exec/src/cancel.rs` + `engine.rs`: abort tasks, delete spill, emit `Timeout`/`Cancelled` within SLA; cancel in-flight internodes via request-id abort (`012`)
- [ ] T039 [US2] Implement multi-shard fan-out invoking `placement::Coordinator` with quorum **per shard** in `crates/exec/src/schedule.rs` / `engine.rs` (FR-010 / `004` FR-053)
- [ ] T040 [US2] Map zero live replicas / missed shard quorum to `PartUnavailable` / `Unavailable` in `crates/exec/src/engine.rs`; honour `partial_ok` (default off) per [coordination.md](contracts/coordination.md) and [data-model.md](data-model.md) §9
- [ ] T041 [P] [US2] Expose `partial` option on PostgreSQL (`SET spacestorage.partial` / hint), Redis (`SS.WITH` / `SS.OPTIONS`), and HTTP headers in handler crates per [query-options-additions.md](contracts/query-options-additions.md); inspect via existing SHOW/OPTIONS surfaces; store `applied.partial_ok` on the record
- [ ] T042 [US2] Forward writes from log-follower coordinators to source domain in `crates/exec/src/engine.rs` (FR-031); `LOCAL_*` write still requires durable source ack; `LOCAL_*` read MAY be stale local; no query-level write serializer in the source domain
- [ ] T043 [US2] Refuse `EACH_QUORUM` (and async-follower levels) unless container opted in — `each_quorum_async{group}` at plan time in `crates/exec/src/planner.rs` (FR-032); queued send ≠ ack
- [ ] T044 [US2] Wait for `013` durability before counting persistent/hybrid write acks toward quorum in `crates/exec/src/engine.rs` (FR-033); memory-mode acks only for memory-mode containers
- [ ] T045 [US2] Implement replica (and later shuffle) ranking in `crates/exec/src/rank.rs` using placement topology + internode RTT/skew gauges (FR-017); call from schedule when placing tasks
- [ ] T046 [US2] Ensure write that cannot meet quorum never reports success; empty successful shard remains zero rows, not unavailability ([coordination.md](contracts/coordination.md))

**Checkpoint**: timeout/cancel/unavailability/forward conformance green. Quickstart §§2 and 5 pass. US2 complete — still need US4 admission/spill before first-binary MVP STOP.

---

## Phase 5: User Story 3 - Transactions with closed isolation set (Priority: P2)

**Goal**: Complete-product SQL `BEGIN`/`COMMIT`/`ROLLBACK` with isolation ∈ {`READ COMMITTED` (default), `SNAPSHOT` where supported}. `SERIALIZABLE` refused as product non-goal. Cassandra consistency is never rewritten into SQL isolation. Multi-shard commits use `004` 2PC. First-binary dialect still refuses `BEGIN` at the handler.

**Independent Test**: Single-node then two-shard: BEGIN/INSERT/COMMIT, ROLLBACK, concurrent READ COMMITTED readers, distributed commit with one participant stopped mid-commit (`004` resolution).

### Tests for User Story 3 ⚠️

- [ ] T047 [P] [US3] Add `crates/conformance/tests/isolation.rs` covering SC-006: BEGIN/COMMIT visibility; BEGIN/ROLLBACK absent row; default isolation `READ COMMITTED`; `SERIALIZABLE` refused naming allowed set; quorum `QUORUM` + isolation remain independent (FR-034)
- [ ] T048 [P] [US3] Add unit tests in `crates/exec/src/txn.rs` for state machine Open→Preparing→Committed|Aborted, Rollback→Aborted, `RetryableTxn` on deadlock/conflict with no partial commit visible

### Implementation for User Story 3

- [ ] T049 [US3] Implement session `Transaction` state machine in `crates/exec/src/txn.rs` per [isolation-and-txns.md](contracts/isolation-and-txns.md) and [data-model.md](data-model.md) §5; bind txn to one session and one protocol
- [ ] T050 [US3] Apply isolation attach: omitted SQL → `READ COMMITTED`; `SNAPSHOT` only when type has `isolation.snapshot`; else `snapshot_unsupported{type}`; any `SERIALIZABLE` → `NotSupported` with allowed set named — in `crates/exec/src/planner.rs` / `options.rs` (FR-015–FR-016)
- [ ] T051 [US3] Implement `TxnBegin`/`TxnCommit`/`TxnRollback` execution in `crates/exec/src/engine.rs` + complete-product path in `crates/handler-postgresql` (and other SQL handlers); first-binary dialect continues handler not-supported before IR (FR-018 / SC-012)
- [ ] T052 [US3] On multi-participant / `distributed` commit, invoke `placement::txn` 2PC in `crates/exec/src/txn.rs`; unreachable participant → single durable outcome or `InDoubt` within `txn_timeout` (`004` FR-077); no undecided forever (FR-019)
- [ ] T053 [US3] Surface deadlock/abort as protocol retryable transaction error with no partial commit (FR-020) via handler error mapping in `crates/handler-postgresql`
- [ ] T054 [P] [US3] Keep `QueryOptions.quorum` and `QueryOptions.isolation` independent fields with no rewrite path in `crates/exec/src/options.rs` and Cassandra handler (FR-034 / [research.md](research.md) R11)
- [ ] T055 [US3] Gate SQL BEGIN execution and 2PC consume behind `query-distributed` / complete-product profile while keeping types available for unit tests in `crates/exec`

**Checkpoint**: isolation conformance passes on complete-product profile. First-binary still returns not-supported for `BEGIN`.

---

## Phase 6: User Story 4 - Admission, concurrency, prepared statements, and COPY (Priority: P2) 🎯 MVP admission (part 3 of 3) + complete-product COPY

**Goal**: Enforce max concurrent queries per node/namespace and max query memory (spill or reject). Honour planner concurrency when the plan has independent tasks. Prepared already required in US1. **Split**: admission/spill/concurrency limits = first-binary MVP (Session 2026-09-16); COPY in/out = complete-product only.

**Independent Test**: Drive past max concurrent (expect named reject); prepared SELECT (already US1); complete-product COPY in and out a small table; first-binary COPY still not-supported.

### Tests for User Story 4 ⚠️

- [ ] T056 [P] [US4] Add `crates/conformance/tests/admission.rs` covering SC-007: `max_concurrent_per_node 1` → second in-flight query gets `admission_rejected{limit:node,…}` with 0 hang; memory cap reject when `spill off`
- [ ] T057 [P] [US4] Add unit tests in `crates/exec/src/spill.rs` for spill dir `{data_dir}/spill/{exec_id}/` create/cleanup on Done/Error/cancel and `admission_rejected{limit:"spill_disk"}` when disk full

### Implementation for User Story 4 — first-binary (admission/spill)

- [ ] T058 [US4] Implement admission acquire (node slot → namespace slot → memory reservation) before Scheduled in `crates/exec/src/admission.rs` / `schedule.rs`; overflow → `admission_rejected{limit, current, max}`; buffer-full (`001`) → `limit:"buffer"` ([admission.md](contracts/admission.md))
- [ ] T059 [US4] Implement spill-to-disk for spillable blocking operators when `spill on` in `crates/exec/src/spill.rs`; when `spill off`, exceeding `max_memory` rejects immediately (FR-022); spill not restored across restart
- [ ] T060 [US4] Honour `Concurrency::Parallel { degree }` only for independent tasks; cap degree by remaining admission slots; default sequential in `crates/exec/src/schedule.rs` (FR-008)
- [ ] T061 [US4] Reject oversize query text/results with `limit_exceeded{what}` at Received or while producing rows in `crates/exec/src/engine.rs` / handlers (`015` limits)
- [ ] T064 [P] [US4] Ensure config validation fixtures under `specs/005-query-execution/contracts/fixtures/invalid/` fail `spacestorage validate` with the documented codes (quickstart §0)

**Checkpoint (first-binary MVP STOP)**: admission conformance green; US1 + US2 + US4 admission/spill validated. Quickstart admission path works. **STOP and VALIDATE** first-binary MVP here — COPY/BEGIN/US3/US5 remain complete-product / slice 8.

### Implementation for User Story 4 — complete-product (COPY)

- [ ] T062 [US4] Implement `CopyIn`/`CopyOut` for complete-product PostgreSQL in `crates/exec/src/engine.rs` + `crates/handler-postgresql` (`CopyIn` → `Mutate::Insert` batches; `CopyOut` → encoded `Scan`; formats Text/Csv/Binary) per [copy-prepared.md](contracts/copy-prepared.md); types without tabular data → `unsupported_by_type`
- [ ] T063 [US4] Confirm first-binary COPY remains handler not-supported before IR (SC-012) in `crates/handler-postgresql`; gate COPY execution behind complete-product / `query-distributed` as appropriate

**Checkpoint (complete-product COPY)**: COPY works on complete-product; first-binary COPY still refused.

---

## Phase 7: User Story 5 - MapReduce, shuffle, aggregation, join, and subscribe (Priority: P3)

**Goal**: Join (inner/left), aggregation (COUNT/SUM/MIN/MAX/AVG + GROUP BY), MapReduce, and shuffle over `internode`. Subscribe-on-results via one job id + SQL/CQL/Redis/HTTP/admin poll (not `LISTEN`/`NOTIFY`). Ranking applies to shuffle workers. ES aggs beyond documented set → not-supported. Produce `008` FR-006 query-processing increments.

**Independent Test**: Two-node join + aggregate; start async job and wait via SQL poll + admin; kill shuffle peer → named failure or retry within timeout, never hang.

### Tests for User Story 5 ⚠️

- [ ] T065 [P] [US5] Add `crates/conformance/tests/join_aggregate.rs` (`--features query-distributed`) covering SC-009: documented join and GROUP BY aggregation across two nodes with correct results when shards available
- [ ] T066 [P] [US5] Add `crates/conformance/tests/subscribe.rs` (`--features query-distributed`) covering SC-010: job id returned; wait via SQL `spacestorage.job_wait` and admin; cancel stops job; `LISTEN`/`NOTIFY` → not-supported
- [ ] T067 [P] [US5] Add shuffle failure unit/integration test in `crates/exec/src/engines/shuffle.rs` (or conformance): peer kill → retry within timeout or `StageFailed{stage}`, never hang (FR-029)

### Implementation for User Story 5

- [ ] T068 [US5] Implement join engines (Inner, Left; HashJoin if build fits/spills else NestedLoopJoin when point-indexed) in `crates/exec/src/engines/join.rs`; unlisted join kinds → `NotSupported`; EXPLAIN names engine ([engines.md](contracts/engines.md))
- [ ] T069 [P] [US5] Implement aggregation `COUNT`/`SUM`/`MIN`/`MAX`/`AVG` + optional `GROUP BY` with partial agg / shuffle-by-key / final agg in `crates/exec/src/engines/aggregate.rs`; HAVING as post-filter `Expr`
- [ ] T070 [P] [US5] Implement MapReduce task emission in `crates/exec/src/engines/mapreduce.rs` and shuffle RPCs `ShuffleOffer`/`ShufflePush`/`ShufflePull`/`StageAbort` as additive kinds in `crates/internode` consumed by `crates/exec/src/engines/shuffle.rs` (FR-027 — not a client/admin port)
- [ ] T071 [US5] Apply `RankKey` placement to shuffle workers among quorum-satisfying candidates in `crates/exec/src/rank.rs` + schedule (FR-017 / US5 acceptance 5)
- [ ] T072 [US5] Implement subscribe/job lifecycle in `crates/exec/src/engines/subscribe.rs`: `async_job` → `Accepted{exec_id}`, background run, `JobState` machine; cancel any wait cancels execute token ([subscribe.md](contracts/subscribe.md))
- [ ] T073 [P] [US5] Wire SQL wait surfaces in `crates/handler-postgresql`: `SET spacestorage.async`, `spacestorage.job_submit` / `job_wait` / `job_cancel`, `spacestorage.jobs`; refuse `LISTEN`/`NOTIFY` as wait path (`015`)
- [ ] T074 [P] [US5] Wire Redis `SS.JOB SUBMIT|GET|WAIT|CANCEL` in `crates/handler-redis` and HTTP `/_spacestorage/jobs/{id}` (+ wait/cancel) on applicable HTTP handlers per [subscribe.md](contracts/subscribe.md)
- [ ] T075 [P] [US5] Complete admin/CLI jobs in `crates/node`, `crates/admin-proto`, `crates/spacestorage`: `GET /v1/jobs`, `{id}`, `{id}/watch`, `POST …/cancel`, `spacestorage jobs wait|cancel` — same `ExecId` as SQL/Redis/HTTP
- [ ] T076 [US5] Refuse Elasticsearch aggregations beyond `terms` + metric min/max/sum/avg/value_count as not-supported with no silent approximation in planner/handler path (FR-035); ES search MUST verbs still lower to shared `LogicalRequest` with `engine=planner` when the handler exists (see T084–T086 / `016` slice 6 — no second engine)
- [ ] T077 [US5] Increment `008` FR-006 query-processing figures from `crates/exec` via `node` stats hooks (totals, errors, in-flight, rows, histograms, retries, scan kind) with required labels when known; **do not** invent parallel `exec_*` public names ([metrics.md](contracts/metrics.md), FR-030)

**Checkpoint**: `cargo test -p spacestorage-conformance --features query-distributed` join/subscribe pass. Quickstart §§6–7 pass.

---

## Phase 8: Polish & Cross-Cutting Concerns

**Purpose**: Docs, operator validation, cleanup across stories

- [ ] T078 [P] Align `docs/query-execution.md` and `docs/examples/query.conf` with [quickstart.md](quickstart.md) and final option/syntax tables
- [ ] T079 [P] Extend CLI help for `spacestorage executions`, `explain`, `jobs`, `query-stats` in `crates/spacestorage` to match admin surfaces
- [ ] T080 Run full [quickstart.md](quickstart.md) validation (config fixtures, planner smoke, timeout/cancel, isolation refuse, admission, three-node partial/forward, EXPLAIN, optional distributed join/job wait) and fix gaps
- [ ] T081 [P] `rustfmt` / `clippy` clean pass on `crates/exec`, new conformance tests, and touched handler/config/node files
- [ ] T082 Confirm `002` tests that assumed `LocalEngine` nested-loop join either keep `LocalEngine` or run under `query-distributed` without forcing internodes on parser-only tests
- [ ] T083 Sweep metrics after a smoke query: assert `008` series exist on `/metrics` for implemented paths (first-binary labels only where applicable)

### Deferred complete-product IR routing (FR-002 / US1 A6 — `016` slice 6)

> **Not first-binary.** When slice-6 handlers land, each MUST lower wire requests to shared `LogicalRequest` and execute with `engine=planner` (extend T027/T076 — do **not** invent a second engine). First-binary MVP is not blocked on these.

- [ ] T084 [P] [US1] When `crates/handler-cassandra` exists (`016` slice 6), lower CQL MUST-subset verbs to `LogicalRequest` and execute through `PlannerEngine` (`engine=planner`) — same IR ownership as PG/Redis (FR-002 / US1 A6)
- [ ] T085 [P] [US1] When `crates/handler-clickhouse` and `crates/handler-elasticsearch` exist (`016` slice 6), lower ClickHouse SQL and Elasticsearch MUST-subset search/ops to `LogicalRequest` through `PlannerEngine` (FR-002 / US1 A6; ES agg ceiling remains T076)
- [ ] T086 [P] [US1] When `crates/handler-s3` and `crates/handler-webdav` exist (`016` slice 6), lower S3/WebDAV MUST-subset GET/PUT (and documented verbs) to `LogicalRequest` through `PlannerEngine` (FR-002 / US1 A6)

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Setup — **BLOCKS** all user stories
- **User Stories (Phase 3–7)**: All depend on Foundational
  - US1 (P1) 🎯 MVP part 1 — shared planner CRUD; then US2 + US4 admission before first-binary STOP
  - US2 (P1) 🎯 MVP part 2 — coordination; builds on US1 execute path; first-binary owed
  - US3 (P2) — txns; needs US1 (+ US2 durability/forward for distributed commit scenarios); complete-product / slice 8
  - US4 (P2) — **split**: admission/spill 🎯 MVP part 3 (after Foundational; can parallel US2); COPY = complete-product dialect only
  - US5 (P3) — engines/subscribe; needs US2 ranking + timeout and US4 spill/admission for heavy plans
- **Polish (Phase 8)**: Depends on stories intended for the release slice; T084–T086 wait on `016` slice 6 handlers

### User Story Dependencies

- **User Story 1 (P1) 🎯 MVP part 1**: After Foundational only. Independently testable via planner smoke + equivalence. Not sufficient alone for first-binary MVP.
- **User Story 2 (P1) 🎯 MVP part 2**: After Foundational; practically after US1 execute path exists. Independently testable via timeout/unavailability/forward suites. First-binary owed (Session 2026-09-16).
- **User Story 3 (P2)**: After Foundational; distributed-commit scenarios need US2 forward/durability + `004` 2PC. Independently testable on complete-product profile.
- **User Story 4 (P2)**: After Foundational; **admission/spill** = first-binary MVP part 3 (can overlap US2); **COPY** after complete-product handler dialect; prepared Bound already in US1. Independently testable via admission suite + COPY round-trip.
- **User Story 5 (P3)**: After Foundational; needs US2 ranking/timeout and preferably US4 admission/spill. Independently testable with `--features query-distributed`.

### Within Each User Story

- Tests (where listed) MUST be written and FAIL before implementation
- Models/types (Phase 2) before services/engines
- Plan/schedule before protocol surface wiring
- Story checkpoint before treating the next priority as done (stories may still be staffed in parallel when `[P]` allows)

### Parallel Opportunities

- T003–T005 after T001/T002
- T007–T013 after T006 starts (different files)
- T019–T022 all `[P]` once Foundational completes
- T033–T036 in parallel once US1 engine path exists
- T047–T048 and T056–T057 can proceed while US2 finishes if types exist
- T068–T070 and T073–T075 marked `[P]` across different crates/files after shuffle/job types exist
- US3 and US4 can be staffed in parallel after US1; US5 after US2 ranking + timeout

---

## Parallel Example: User Story 1

```bash
# Launch all US1 tests together (must fail until planner exists):
Task: "T019 planner_smoke.rs SC-001/SC-012"
Task: "T020 equivalence.rs SC-002"
Task: "T021 planner type-refuse unit tests"
Task: "T022 explain.rs SC-008"

# Then implementation (order: stages → type match → execute → prepared/EXPLAIN → handlers):
Task: "T023–T026 planner/schedule/engine CRUD"
Task: "T027–T028 Bound + Explain"
Task: "T029–T031 dialect gates + Redis + admin records"
```

## Parallel Example: User Story 2

```bash
Task: "T033 timeout_cancel.rs"
Task: "T034 unavailability.rs"
Task: "T035 forward_local.rs"
Task: "T036 rank.rs unit tests"

Task: "T037–T040 quorum/cancel/fan-out/partial"
Task: "T041 partial option surfaces [P]"
Task: "T042–T046 forward, EACH_QUORUM, durable acks, ranking"
```

## Parallel Example: User Story 5

```bash
Task: "T065 join_aggregate.rs"
Task: "T066 subscribe.rs"
Task: "T067 shuffle failure test"

Task: "T068 join.rs | T069 aggregate.rs | T070 mapreduce+shuffle RPCs"
Task: "T073 SQL jobs | T074 Redis/HTTP jobs | T075 admin/CLI jobs"
Task: "T076 ES ceiling | T077 metrics increments"
```

---

## Implementation Strategy

### MVP First (User Story 1 + US2 + US4 admission/spill)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (**CRITICAL** — blocks all stories)
3. Complete Phase 3: User Story 1 (shared planner CRUD + prepared + optional EXPLAIN)
4. Complete Phase 4: User Story 2 (timeout/quorum/cancel/unavailability/forward/ranking)
5. Complete Phase 6 admission/spill (T056–T061, T064) — US4 first-binary half
6. **STOP and VALIDATE** first-binary MVP: `planner_smoke` + equivalence + timeout/cancel/unavailability + admission + quickstart first-binary paths; `engine=planner`
7. Demo/ship first-binary path **with** timeout, quorum, cancel, unavailability, and admission — without MapReduce/BEGIN/COPY/subscribe (US3, US4 COPY, US5 remain complete-product / slice 8)

### Incremental Delivery

1. Setup + Foundational → types and `PlannerEngine` seam ready
2. US1 → shared planner CRUD (MVP part 1)
3. US2 → fault-tolerant coordination (timeout/cancel/unavailability/forward/ranking) (MVP part 2)
4. US4 admission/spill (can overlap late US2) → hard limits live (MVP part 3) — **first-binary STOP**
5. US3 txns + US4 COPY on complete-product
6. US5 join/agg/MapReduce/subscribe + metrics → slice 8 / complete acceptance
7. T084–T086 when `016` slice 6 handlers exist → complete-product IR routing (FR-002 / US1 A6)
8. Each story adds value without requiring a second protocol engine

### Parallel Team Strategy

1. Team completes Setup + Foundational together
2. After Foundational:
   - Developer A: US1 then US2
   - Developer B: US4 admission/spill (then COPY on complete-product)
   - Developer C: US3 txn types/tests (integrate after US2 durability)
3. After US2 + US4 admission: staff US5 engines vs subscribe surfaces in parallel; T084–T086 when handlers exist

### Suggested wave order (from plan.md)

1. IR + `PlannerEngine` CRUD + stages (US1 / Foundation)
2. Admission, cancel, timeout, size limits, partial option (US2 + US4 admission) — first-binary MVP wave
3. Live `PlacementInfo` + fan-out + unavailability + clamp/reject + forward + ranking (US2)
4. EXPLAIN + prepared bind (US1)
5. Isolation + BEGIN/ROLLBACK + 2PC consume (US3)
6. COPY (US4 complete-product)
7. Join/aggregate (US5)
8. MapReduce/shuffle/subscribe + metrics sweep (US5 + Polish)
9. Complete-product handler IR routing (T084–T086 / `016` slice 6)

---

## Notes

- [P] = different files, no dependencies on incomplete tasks
- [USn] maps to spec user stories for traceability
- Handlers MUST NOT depend on `exec` internals — only `QueryEngine` + `LogicalRequest`
- First-binary MVP = US1 + US2 + US4 admission/spill; first-binary dialect short-circuits COPY/BEGIN in the handler before `exec`
- `LocalEngine` remains for RF=1 / `002` tests; default production path is `PlannerEngine`
- Spill dirs are ephemeral under `{data_dir}/spill/<exec-id>/`; not restored across restart
- Commit after each task or logical group; stop at any checkpoint to validate independently
- Avoid: second IR, protocol-private engines, shuffle on client ports, `LISTEN`/`NOTIFY` as job wait, treating quorum as SQL isolation
- Complete-product IR routing for CQL/ClickHouse/ES/S3/WebDAV is T084–T086 (`016` slice 6); not a first-binary gate

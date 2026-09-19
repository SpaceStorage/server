# Implementation Plan: Query Execution, MapReduce, Transactions, and Fault-Tolerant Results

**Branch**: `005-query-execution` | **Date**: 2026-09-16 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/005-query-execution/spec.md` (Clarifications, Sessions 2026-09-15 and 2026-09-16 — including post-plan: job-id poll wait, partial-results option)

## Summary

Replace the interim `LocalEngine` from `002` with the real shared execution stack **behind the same `QueryEngine` trait**. Protocol handlers keep lowering wire requests into the existing canonical IR (`LogicalRequest`). This feature owns the inspectable stages — parse (bind/prepare), plan, schedule, execute — plus admission, cancel/timeout, unavailability, source-domain write forwarding, replica/shuffle ranking (topology ladder then RTT / HLC-skew health), a closed isolation set, and (for the complete product / `16` slice 8) join, aggregation, MapReduce, shuffle over `internode`, subscribe-on-results, SQL `BEGIN`, COPY, and L1 2PC.

It does **not** add a second engine per protocol, a client-facing query port, or a second IR. Fan-out, quorum arithmetic, durable-ack filtering, `EACH_QUORUM` refuse-by-default, and 2PC stay in `004`/`placement`; this planner **invokes** them. Internode framing and RTT/skew publication stay in `012`; this stack is a client of those RPCs for shuffle and ranking. Subscribe wait is a **job id** plus documented SQL/CQL poll (not `LISTEN`/`NOTIFY`), Redis command, HTTP path, and admin — one job object. Partial results require an explicit option in the same family as timeout/quorum; default **off**.

First-binary (`16` slices 1–5): auto-commit PostgreSQL DML/DDL + extended/prepared and Redis MUST-list on `K/V Store` through `PlannerEngine`. `COPY`/`BEGIN` remain not-supported on that dialect profile. Slice 8 lands join/agg/MapReduce/subscribe/distributed transactions and complete-product `BEGIN`/`COPY`.

## Technical Context

**Language/Version**: Rust 1.87 (edition 2024, MSRV 1.85), same Cargo workspace as `001`–`016`.

**Primary Dependencies**: existing workspace crates only. `sqlparser` (PostgreSQL + ClickHouse dialects) already in `002` handler crates — parsers stay there; this crate consumes `LogicalRequest`. `tokio` / `async-trait` / `serde` / `tracing` / `uuid` / `bytes` / `parking_lot`. Spill-to-disk uses `tokio::task::spawn_blocking` + the node's `data_dir` spill path (no extra crate). No `datafusion`, no `arrow` as a type system, no gRPC, no `*-sys`. Ranking and 2PC are function calls into `placement` / `internode`.

**Storage**: No new user-data format. Execution records in the existing `exec.records` ring (`001`/`002`). Spill files under `{data_dir}/spill/<exec-id>/` (ephemeral; deleted on Done/Error/cancel; not restored across restart). Transaction and 2PC participant state live in the `004` placement catalog (`txn.rs`). Subscription handles in an in-memory map replicated as internodes `JobStatus` messages (not a second catalog). WAL durability of write acks remains `013`.

**Testing**: `cargo test`. Unit tests in `exec` (plan stages, isolation refuse, admission, clamp/reject, ranking key order, spill/reject). `crates/conformance` gains: first-binary planner smoke (SC-001/SC-012), cross-protocol equivalence (SC-002), timeout/cancel (SC-003/SC-004), unavailability (SC-005), admission (SC-007), EXPLAIN (SC-008, complete-product), follower-forward / LOCAL_ONE / clamp (SC-011), SERIALIZABLE refuse (SC-006). Slice-8 suite (feature `query-distributed`): join/agg two-node (SC-009), subscribe (SC-010), 2PC killed participant (delegates to `004` SC-015), shuffle peer kill. Contract tests on `contracts/fixtures/`.

**Target Platform**: Linux x86_64/aarch64 servers (primary), macOS for development. Multi-node tests: in-process nodes on loopback internodes, same harness as `004`/`016`.

**Project Type**: Cargo workspace extension — **no new binary**. Grow `crates/exec` from seam to engine; additive internodes RPC kinds in `crates/internode`; config/admin/CLI surfaces; `release-profile` already gates slice 8.

**Performance Goals**: first-binary PG INSERT/SELECT loopback ≥ 20 k simple queries/s per coordinator (same order as `004` point write); timeout error within deadline + 1 s (target + 50 ms, inherited from `002` SC-005); cancel stops resource use in that window; EXPLAIN of a single-table SELECT < 10 ms p95 without touching data; admission reject < 1 ms; shuffle retry does not hang past the query timeout.

**Constraints**: one `QueryEngine` implementation is live per node (`PlannerEngine`); handlers MUST NOT depend on `exec` internals (only the trait + `LogicalRequest`). Default concurrency is sequential. Isolation ∈ {`READ COMMITTED`, `SNAPSHOT`}; `SERIALIZABLE` → `NotSupported`. Defaults clamp / explicit reject for quorum (`002`). `LOCAL_*` = coordinator `quorum_domain`. Writes on followers forward to source. `EACH_QUORUM` refuse unless container opted in. Persistent/hybrid write acks wait for `013`. Shuffle uses `internode` only. No timeout hardcoded for a short RTT. First-binary dialect forbids COPY/BEGIN before planning.

**Scale/Scope**: IR variants already in `002` plus Transaction/Explain/Copy/Subscribe/Isolation/Concurrency attachments; four inspectable stages; admission; ranking; ~8 internodes shuffle/job RPCs; join (inner/left), aggregation (COUNT/SUM/MIN/MAX/AVG + GROUP BY), MapReduce, spill. Roughly: planner+record (~3 k), admission+txn (~2 k), engines+shuffle (~4 k), config/admin (~1 k), conformance (~3 k); ≈ 12–16 k lines including tests. First-binary path is the CRUD subset (~half).

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Gate | Status |
|---|-----------|------|--------|
| I | Rust-Only Stack | No new C-backed crates. No DataFusion/Arrow as a second type system. Spill is std + Tokio blocking pool | PASS |
| II | Fully Asynchronous Tokio Runtime | Execute/schedule/shuffle are `async`. Sort/hash/spill on the existing bounded `spawn_blocking` pool. Cancel drops the stream; no worker-thread block | PASS |
| III | Single-Process Multithreaded Monolith | `exec` remains a library in `spacestoraged`. No query sidecar, no extra binary | PASS |
| IV | Type-Driven Multiparadigm | Planner matches `003` operation sets; unsupported ops refused before data. No protocol-private engine. L4 walk is `003` rules | PASS |
| V | Protocol Compatibility on Distinct Ports | No new client port. Shuffle/job RPCs on existing `internode`. Complete-product handlers still `002`/`015`; first binary PG+Redis only (`016`) | PASS |
| VI | Every Node Is a Request Coordinator | Planner runs on the receiving node. Fan-out is `004` coordinator, not a sticky query leader. Follower writes forward to source (`012`) | PASS |
| VII | Label-Based Planetary Placement | Replica/shuffle ranking uses the cluster topology ladder, then RTT/HLC-skew (`004` FR-001b). Ladder is not a voting set | PASS |
| VIII | Cassandra-Style Quorum with Protocol Defaults | Every query carries timeout+quorum; defaults write TWO / read ONE; clamp vs explicit reject; durable source-domain acks; `EACH_QUORUM` refuse-by-default | PASS |
| IX | Multi-Tenant Namespaces | Admission per node and per namespace; records and metrics labelled with namespace | PASS |
| X | Observability as a Product Surface | Produces `08` FR-006 query-processing figures; names stay in `08`; no required label dropped | PASS |
| XI | Documented, Expandable Configuration | `query { }` block with documented defaults and starter examples | PASS |
| XII | Raft Controller Elections and Local Restore | Does not run Raft. Consumes `004` 2PC and `006` leadership seam when a plan needs them. Spill is not restored across restart | PASS |
| XIII | Security Defaults for Data and Roles | No new authenticator. Assumes `002` already authorized the session (`014`). Spill files inherit `data_dir` permissions | PASS |
| Arch. Contracts | Adapter ≠ type system; quorum/timeout/namespace everywhere | Handlers still only call `QueryEngine`. IR stays `LogicalRequest` | PASS |
| Observability Contract | No `08` series renamed | Additions of values on existing query families only | PASS |

**Gate result (pre-research)**: PASS. Sequencing (slice 8 engines after first-binary CRUD) is the purpose of `016` and is recorded as deferred, not a cancelled MUST.

## Project Structure

### Documentation (this feature)

```text
specs/005-query-execution/
├── plan.md                         # This file
├── research.md                     # Phase 0: R1–R19
├── data-model.md                   # Phase 1: IR, plan, record, txn, task, subscription, admission
├── quickstart.md                   # Phase 1: planner smoke + timeout/cancel + partial + job wait
├── contracts/
│   ├── ir.md                       # LogicalRequest is the IR; extensions; MUST NOT verbs
│   ├── planner-executor.md         # stages, physical plan, EXPLAIN, type match
│   ├── admission.md                # concurrent queries, memory, spill/refuse
│   ├── isolation-and-txns.md       # closed set, BEGIN, 2PC consume, SERIALIZABLE
│   ├── coordination.md             # clamp/reject, fan-out, forward, EACH_QUORUM, ranking, unavailability, partial option
│   ├── engines.md                  # join, aggregate, MapReduce, shuffle RPCs
│   ├── subscribe.md                # job id + SQL/CQL/Redis/HTTP/admin wait (not LISTEN)
│   ├── query-options-additions.md  # partial-results + async_job in the 002 option family
│   ├── copy-prepared.md            # COPY, extended/prepared
│   ├── config-directives.md        # query { } + CLI/admin
│   ├── metrics.md                  # 08 FR-006 figures this crate must increment
│   └── fixtures/
│       ├── README.md
│       ├── query-block.conf
│       └── invalid/
├── checklists/requirements.md
└── tasks.md                        # Phase 2 (/speckit-tasks — NOT created here)
```

### Source Code (repository root)

```text
crates/
├── exec/                                    # spacestorage-exec — CORE (002 seam, now the engine)
│   └── src/
│       ├── lib.rs                           # QueryEngine trait (unchanged signatures + additive methods)
│       ├── request.rs                       # LogicalRequest + Txn/Explain/Copy/Subscribe attachments
│       ├── options.rs                       # QueryOptions + IsolationLevel + Concurrency + PartialOk
│       ├── record.rs                        # ExecutionRecord stages, applied options, replicas
│       ├── error.rs                         # ExecError: Timeout, Cancelled, Unavailable, NotSupported, Admission, Isolation
│       ├── local/                           # LocalEngine retained for unit tests / RF=1 without internodes
│       ├── planner.rs                       # logical → physical; type catalog match; EXPLAIN
│       ├── schedule.rs                      # Task graph, concurrency, admission tokens
│       ├── engine.rs                        # PlannerEngine: default QueryEngine
│       ├── admission.rs                     # per-node / per-namespace semaphores; memory cap
│       ├── txn.rs                           # session txn; isolation; invokes placement::txn 2PC
│       ├── cancel.rs                        # disconnect + explicit cancel + timeout
│       ├── rank.rs                          # ladder then RTT/skew (reads placement + internode gauges)
│       ├── spill.rs                         # spill dir lifecycle
│       └── engines/                         # slice 8; compiled behind feature query-distributed
│           ├── join.rs  aggregate.rs  mapreduce.rs  shuffle.rs  subscribe.rs
│
├── internode/                               # additive RPC: ShuffleOffer/Push/Pull, JobStatus, SubscribeNotify
├── placement/                               # consumed: Coordinator, quorum, ranking inputs, 2PC (no new crate)
├── config/                                  # query { max_concurrent_*; max_memory; spill; default_concurrency }
├── node/                                    # PlannerEngine wiring; query stats; admin executions/explain/jobs
├── admin-proto/                             # ExecutionRecord stage fields; Subscription; ExplainPlan DTOs
├── spacestorage/                            # CLI: executions, explain, jobs, query-stats
├── handler-postgresql/                      # COPY/BEGIN/EXPLAIN/SET isolation+partial+async; job_submit/wait/cancel
├── handler-cassandra/                       # prepared already; isolation N/A; spacestorage.partial payload; jobs table
├── release-profile/                         # slice 8 = query-distributed feature
└── conformance/
    └── tests/
        ├── planner_smoke.rs                 # SC-001, SC-012 (PG+Redis through PlannerEngine)
        ├── equivalence.rs                   # SC-002 (extends 002)
        ├── timeout_cancel.rs                # SC-003, SC-004
        ├── unavailability.rs                # SC-005 (option off fail-whole; option on named partial)
        ├── isolation.rs                     # SC-006 SERIALIZABLE refuse; BEGIN on complete-product
        ├── admission.rs                     # SC-007
        ├── explain.rs                       # SC-008
        ├── forward_local.rs                 # SC-011
        ├── join_aggregate.rs                # SC-009 (query-distributed)
        └── subscribe.rs                     # SC-010 (query-distributed)

docs/
├── query-execution.md                       # stages, isolation, admission, ranking
└── examples/query.conf                      # copy of query-block.conf
```

**Structure Decision**: Keep `crates/exec` as the crate handlers already depend on so `002` does not change its dependency graph. Replace `LocalEngine` as the **node default** with `PlannerEngine`; keep `LocalEngine` for RF=1 tests. Do not add `crates/query` or a planner binary. Slice-8 engines live in `exec/src/engines/` behind Cargo feature `query-distributed`, which `release-profile` complete-product enables. Shuffle bytes move on `internode` (existing handler), not a new port.

## Complexity Tracking

| Violation / deviation | Why Needed | Simpler Alternative Rejected Because |
|-----------------------|------------|-------------------------------------|
| Slice 8 engines deferred in the first binary | `016` slice order: CRUD through this stack now; joins/MapReduce/subscribe/2PC/BEGIN after three-node quorum and remaining handlers | Blocking first-binary PG on full MapReduce inverts slices and delays the shippable binary. Complete-product MUSTs remain in this spec. |
| `LocalEngine` retained alongside `PlannerEngine` | RF=1 unit tests and `002` local conformance must keep a zero-internode path | Deleting `LocalEngine` would force internodes even for parser tests. Default production path is `PlannerEngine`. |
| Spill on `spawn_blocking` to local disk | FR-022 memory cap | Unbounded memory (rejected by `15`); a second process for spill (rejected by Principle III). |
| SQL parsers remain in handler crates | `002` already lowers to IR; one IR owner is this crate. **Parser-stage ownership (FR-005)**: handler parse → IR is stage `Received`/`Bound` ownership in the handler crate; `ExecutionRecord` always names shared planner stages thereafter (`Planned`|`Scheduled`|`Running`|…) — never a private engine. T019/T032 assert those shared stage names on the record. | Moving `sqlparser` into `exec` would pull protocol dialects into the engine and invite a second AST. |

## Phase 0 — Research

See [research.md](research.md). All Technical Context items were resolved from the spec, constitution, sibling plans `002`/`004`/`012`/`015`/`016`, and clarify 2026-09-16 (job-id poll, partial-results option). No `NEEDS CLARIFICATION` remains.

## Phase 1 — Design

- [data-model.md](data-model.md): IR attachments, Logical/Physical plan, ExecutionRecord stages, Task, Transaction, Subscription, Admission, SpillFile, RankKey; `partial_ok`/`async_job` as `Sourced<bool>`.
- [contracts/](contracts/): eleven contracts plus fixtures covering FR-001–FR-035 and SC-001–SC-012 (job-id poll wait; partial-results option).
- [quickstart.md](quickstart.md): first-binary planner smoke, timeout/cancel, unavailability + partial option, then optional two-node join and SQL job wait when `query-distributed` is on.

## Constitution Check (post-design)

Re-evaluated after Phase 1: no new violations. Design keeps a single `QueryEngine`, shuffle on `internode`, leaderless coordination, closed isolation set, durable source-domain write acks, ladder+RTT ranking, and `08` additive metrics. Slice 8 remains deferred in first-binary builds via Cargo feature, not by deleting FRs. **Gate result: PASS.**

## Risks and sequencing notes

- Suggested `/speckit-tasks` waves: (1) IR extensions + `PlannerEngine` CRUD replacing default `LocalEngine` + stages on `ExecutionRecord`; (2) admission, cancel, timeout, size limits, partial-results option; (3) live `PlacementInfo` + fan-out + unavailability + clamp/reject + forward-to-source + ranking; (4) EXPLAIN + prepared bind; (5) isolation + BEGIN/ROLLBACK (complete-product dialect) + 2PC consume; (6) COPY; (7) join/aggregate; (8) MapReduce/shuffle/subscribe (job-id poll on SQL/CQL/Redis/HTTP + admin) + metrics sweep.
- `002` tests that assumed `LocalEngine` nested-loop join must run against `query-distributed` or keep `LocalEngine` in those tests only.
- First-binary dialect (`016`) must still short-circuit COPY/BEGIN in the handler **before** `exec` so this crate can implement COPY/BEGIN for complete-product without accidentally enabling them in slices 1–5.
- `004` coordinator remains the fan-out path; the planner must not open its own TCP to replicas.
- Spill directories must be quota-accounted (`07`) later; first implementation counts against `max_query_memory` + node disk, rejects if spill disk is full.

## Next Step

Run `/speckit-tasks` to generate `tasks.md` from this plan and the Phase 1 artifacts.

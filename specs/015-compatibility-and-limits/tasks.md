---
description: "Task list for protocol compatibility ceiling, limits, isolation, and rolling upgrade"
---

# Tasks: Protocol Compatibility Ceiling, Limits, Isolation, and Rolling Upgrade

**Input**: Design documents from `/specs/015-compatibility-and-limits/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md), [data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: Requested. Spec Independent Tests + SC-001–SC-005 + plan Testing section (`cargo test -p spacestorage-compat`, `cargo test -p spacestorage-conformance` with first-binary / `handlers-complete` / `query-distributed` features, contract fixtures under `contracts/fixtures/`). Write failing tests first where listed.

**Scope of this feature**: Own the **compatibility ceiling** as `crates/compat` (`spacestorage-compat`): MUST/MUST NOT matrix, wire versions, closed isolation set, size/connection/admission limit types+defaults, product version window N/N+1. Handlers (`002`) call `classify` before IR; `exec` (`005`) consumes isolation + admission/spill policy; `types` (`003`) grows `snapshot_capable` / `introduced_in`; `internode`/`storage` (`012`/`013`) import the window predicate. Do **not** reimplement protocol handlers or the planner. Never brand the matrix “v1”.

**Sibling crates** (wire into; do not duplicate internals): `crates/handler-postgresql`, `crates/handler-redis`, `crates/handler-cassandra`, `crates/handler-elasticsearch`, `crates/handler-clickhouse`, `crates/handler-s3`, `crates/handler-webdav`, `crates/protocol-core`, `crates/exec`, `crates/types`, `crates/internode`, `crates/storage`, `crates/config`, `crates/node`, `crates/release-profile`, `crates/conformance`.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: [US1], [US2], [US3] on user-story phase tasks only
- Every task includes an exact file path

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Workspace member and crate skeleton for `spacestorage-compat` per [plan.md](plan.md) Project Structure

- [ ] T001 Create `crates/compat/Cargo.toml` (package `spacestorage-compat`, edition 2024, deps `serde`, `serde_json`, `bytes`, `tracing`, `thiserror` as needed; no `*-sys`, no DataFusion, no protocol crates) and `crates/compat/src/lib.rs` that `mod`s `matrix`, `wire`, `isolation`, `limits`, `version`, `profile`
- [ ] T002 Add `crates/compat` to workspace `[workspace.members]` in `Cargo.toml` and ensure `handler-*`, `exec`, `types`, `internode`, `storage`, `config`, `node`, `release-profile`, and `conformance` can depend on `spacestorage-compat` without cyclic deps (handlers/exec → compat; compat MUST NOT depend on handlers or exec)
- [ ] T003 [P] Copy [contracts/fixtures/first-binary.conf](contracts/fixtures/first-binary.conf) and [contracts/fixtures/limits-raised.conf](contracts/fixtures/limits-raised.conf) into `docs/examples/compat/` (or keep referenced from specs) and document the invalid fixtures path in `crates/compat/README.md`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Shared types, defaults, and named-error vocabulary every story uses. No user-story work until this phase is complete.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [ ] T004 Implement `DialectProfile { FirstBinary, HandlersComplete, CompleteProduct }` in `crates/compat/src/profile.rs` with subset relation FirstBinary ⊂ HandlersComplete ⊂ CompleteProduct; never name a profile `"v1"`; export `fn active_profile() -> DialectProfile` from `release-profile` feature mapping (`first-binary` → FirstBinary; `handlers-complete` → HandlersComplete; `query-distributed` / complete → CompleteProduct) in `crates/release-profile/src/profile.rs` (or a thin re-export in `crates/compat/src/profile.rs`)
- [ ] T005 [P] Implement `ProtocolId` and `WireVersion { protocol, label: &'static str }` constants in `crates/compat/src/wire.rs` exactly as [wire-versions.md](contracts/wire-versions.md): PostgreSQL `"3.0"`; Cassandra `"v4"` and `"v5"`; Redis `"RESP2"`; Elasticsearch `"HTTP/1.1"`; ClickHouse native + ClickHouseHttp; S3 SigV4; WebDAV RFC 4918 subset; RESP3 absent from all current profiles
- [ ] T006 [P] Implement `VerbClass { Must, MustNot }` and `fn classify(profile: DialectProfile, protocol: ProtocolId, verb: &Verb) -> VerbClass` skeleton in `crates/compat/src/matrix.rs` (unknown verb → `MustNot`); leave per-protocol tables empty until US1 fills them from [matrix.md](contracts/matrix.md)
- [ ] T007 [P] Implement `IsolationLevel { ReadCommitted, Snapshot }` and `IsolationRefuse` codes in `crates/compat/src/isolation.rs` with `map_sql_isolation` table from [data-model.md](data-model.md) §4 / [isolation.md](contracts/isolation.md): omitted/`read committed` → `ReadCommitted`; `repeatable read`/`snapshot` → `Snapshot`; `read uncommitted` → `ReadCommitted` (documented upgrade); `serializable` → refuse `serializable_nongoal`
- [ ] T008 [P] Implement `Limits` struct and `LimitKind` enum in `crates/compat/src/limits.rs` with defaults from [limits.md](contracts/limits.md) / [data-model.md](data-model.md) §5: `max_key` default **1 KiB**; `max_value` default **16 MiB**; `max_query_text` default **1 MiB**; `max_result` default **64 MiB**; `max_connections_per_entrypoint` default **10_000**; `max_connections_per_principal` default **1_000**; `LimitKind` includes `Key`, `Value`, `QueryText`, `Result`, `ConnectionsEntrypoint`, `ConnectionsPrincipal`, `ConcurrentQueriesNode`, `ConcurrentQueriesNamespace`, `QueryMemory`, `Buffer`, `SpillDisk`
- [ ] T009 [P] Implement `AdmissionPolicy { spill: SpillMode }` and `SpillMode { Off, On }` in `crates/compat/src/limits.rs`: FirstBinary/HandlersComplete default `Off`; CompleteProduct default `On` per [research.md](research.md) R4; document that concurrent-query overflow always rejects and MUST NOT spill a slot
- [ ] T010 [P] Implement `ProductVersion(u16)` with first binary = **1** and `fn peers_ok(local, peer) -> bool` as `abs_diff <= 1` in `crates/compat/src/version.rs` per [version-window.md](contracts/version-window.md)
- [ ] T011 Export named error codes in `crates/compat/src/lib.rs` (or `error.rs`) exactly as [data-model.md](data-model.md) §9: `compat_must_not{protocol,verb}`, `serializable_nongoal`, `snapshot_unsupported{type}`, `limit_exceeded{limit,current,max}`, `admission_rejected{limit,current,max}`, `product_version_window{local,peer}`, `format_too_new{have,need}`, `type_too_new{type,introduced_in,local}`, `limits_zero{knob}`, `copy_not_in_profile`, `begin_not_in_profile`, `cursor_not_in_profile`, `agg_not_in_profile`
- [ ] T012 Parse `limits { … }` and optional `cluster { product_version N; }` in `crates/config/src/` per [config-directives.md](contracts/config-directives.md): all limits fields optional (missing → defaults); zero → `limits_zero{knob}`; unparsable unit → `limits_unknown_unit`; `product_version 0` → `product_version_zero`; live-reload applies to **new** connections/queries only; effective config reports provenance `configured | built_in`
- [ ] T013 Add unit tests in `crates/compat/src/` covering: default `Limits` values; `limits_zero` reject path via config fixture [contracts/fixtures/invalid/limits-zero.conf](contracts/fixtures/invalid/limits-zero.conf); `product_version_zero` via [contracts/fixtures/invalid/product-version-zero.conf](contracts/fixtures/invalid/product-version-zero.conf); `peers_ok(1,2)==true`, `peers_ok(1,3)==false`; `map_sql_isolation("serializable")` → `serializable_nongoal`

**Checkpoint**: `cargo test -p spacestorage-compat` compiles; config validates invalid fixtures with exit 2. User stories can start.

---

## Phase 3: User Story 1 - Complete-product verb contract (Priority: P1) 🎯 MVP

**Goal**: Machine-readable MUST/MUST NOT matrix + wire versions; handlers return that protocol’s not-supported form **before** IR for MUST NOT verbs; first-binary PG+Redis subset; slice-6 remaining handlers; slice-8 PG BEGIN/COPY/cursors + ES aggregations. Stock **clients** on MUST verbs. Never silent empty success. Handshake mismatch refuse &lt; 1 s stays `002`.

**Independent Test**: For each profile’s handlers, run MUST smoke; issue one MUST-NOT verb; confirm not-supported. PostgreSQL CP: forward-only `DECLARE`/`FETCH`/`CLOSE` inside a txn; `WITH HOLD` not-supported. ES HC: CRUD + MUST search; aggs not-supported until CP. See [quickstart.md](quickstart.md) §§1,4,5.

### Tests for User Story 1 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T014 [P] [US1] Add unit tests in `crates/compat/src/matrix.rs` (or `crates/compat/tests/matrix.rs`) that `classify` for FirstBinary returns `MustNot` for PG `BEGIN`/`COPY`/`DECLARE` and Redis `CLUSTER`/`EVAL`, and `Must` for PG simple DML and Redis `GET`/`SET` per [matrix.md](contracts/matrix.md)
- [ ] T015 [P] [US1] Add conformance MUST/MUST NOT smokes in `crates/conformance/tests/compat_first_binary.rs` (SC-001/SC-002): stock PG INSERT/SELECT/UPDATE/DELETE + prepared succeed; `COPY`/`BEGIN`/`DECLARE CURSOR` → not-supported (`0A000` / `copy_not_in_profile` / `begin_not_in_profile` / `cursor_not_in_profile`); Redis AUTH/PING/GET/SET/DEL/EXISTS/SCAN/SELECT/TTL succeed; `CLUSTER SLOTS`/`EVAL` → not-supported; 0 silent empty successes
- [ ] T016 [P] [US1] Add `crates/conformance/tests/compat_handlers_complete.rs` behind feature `handlers-complete`: Cassandra/ES/CH/S3/WebDAV MUST smokes; ES search `match`/`term` succeed; ES ILM and aggregations → not-supported (`agg_not_in_profile`); CH dictionaries / S3 versioning / WebDAV LOCK → not-supported; handshake mismatch &lt; 1 s
- [ ] T017 [P] [US1] Add `crates/conformance/tests/compat_complete_product.rs` behind feature `query-distributed`: PG `BEGIN`/`COMMIT`, COPY text/csv/binary, forward-only `DECLARE`/`FETCH`/`CLOSE` succeed; `WITH HOLD`/SCROLL/portal-after-COMMIT → not-supported; ES closed aggregations `terms`/`min`/`max`/`sum`/`avg`/`histogram`/`value_count` succeed; ILM still not-supported

### Implementation for User Story 1

- [ ] T018 [P] [US1] Encode full per-protocol MUST/MUST NOT tables in `crates/compat/src/matrix.rs` from [matrix.md](contracts/matrix.md), including profile columns FB/HC/CP; ClickHouse as two `ProtocolId`s (`ClickHouse`, `ClickHouseHttp`); verbs outside MUST → `MustNot`
- [ ] T019 [P] [US1] Encode PostgreSQL cursor subset in `crates/compat/src/matrix.rs` (or `cursors.rs`) from [postgresql-cursors.md](contracts/postgresql-cursors.md): FB/HC → `cursor_not_in_profile` for SQL `DECLARE`/`FETCH`/`CLOSE`; CP MUST forward-only `DECLARE [NO SCROLL] … [WITHOUT HOLD]`, `FETCH [FORWARD]`, `CLOSE` inside open txn; MUST NOT `WITH HOLD`, `SCROLL`, `FETCH BACKWARD`, use after `COMMIT`
- [ ] T020 [P] [US1] Encode Elasticsearch search/aggregation lists in `crates/compat/src/matrix.rs` (or `elasticsearch.rs`) from [elasticsearch-search.md](contracts/elasticsearch-search.md): MUST search `query_string`|`match`|`term`|`range`|`bool` (clauses of those only); MUST aggregations only in CompleteProduct: `terms`,`min`,`max`,`sum`,`avg`,`histogram`,`value_count`; before CP → `agg_not_in_profile`; ILM/ingest/ML/CCR/`script`/`significant_terms`/`composite`/`date_histogram` → `MustNot`
- [ ] T021 [P] [US1] Encode COPY profile rules in `crates/compat` from [copy.md](contracts/copy.md): FB/HC → `copy_not_in_profile`; CP MUST `text`/`csv`/`binary`; MUST NOT `COPY … PROGRAM`, `FREEZE`, server-path COPY
- [ ] T022 [US1] Wire `classify` **before** `LogicalRequest` in `crates/handler-postgresql/src/` and `crates/handler-redis/src/`: `MustNot` → existing `002` `ErrorRenderer` (PG `0A000`, Redis `-ERR unknown command`); never empty `+OK` / never PG `T`/`D`/`C` for refused verbs ([research.md](research.md) R10)
- [ ] T023 [US1] Wire `classify` before IR in `crates/handler-cassandra/`, `crates/handler-elasticsearch/`, `crates/handler-clickhouse/` (both handlers), `crates/handler-s3/`, `crates/handler-webdav/` for HandlersComplete+ builds; ES MUST NOT never returns 200 + empty hits
- [ ] T024 [US1] Ensure `DialectProfile::FirstBinary` entrypoint naming `elasticsearch`/`cassandra`/… fails startup `unknown_handler` via `crates/config` + `crates/release-profile` (owned seam with `016`); document in `crates/compat` that FB has no ES handler (FR-015)
- [ ] T025 [US1] Increment `spacestorage_compat_must_not_total{protocol,verb}` on MUST NOT paths in handler wiring per [metrics.md](contracts/metrics.md); do not rename `008` labels
- [ ] T026 [US1] For CompleteProduct, implement SQL cursor as a **held portal bound to txn id** (dropped on COMMIT/ROLLBACK/disconnect) in `crates/handler-postgresql/` + `crates/exec/` per [research.md](research.md) R6 — supersedes `002` “cursors beyond portals → not-supported” for CP only

**Checkpoint**: First-binary PG/Redis MUST/MUST NOT conformance green. HC/CP suites green when those features are enabled. Matrix is the single source of truth; no silent empty successes (SC-001/SC-002).

---

## Phase 4: User Story 2 - Isolation, cancel, admission (Priority: P1)

**Goal**: Closed isolation set `READ COMMITTED` / `SNAPSHOT` (on `snapshot_capable` types); `SERIALIZABLE` refused; no silent downgrade; cancel/disconnect stops work; concurrent-query caps hard-reject; query-memory: FB/HC reject; CP spill for sort/hash/agg else reject naming the cap.

**Independent Test**: Default isolation → `READ COMMITTED`; `SET SERIALIZABLE` refuse; SNAPSHOT/REPEATABLE READ on capable type; SNAPSHOT on non-capable → refuse naming type; hit concurrent max → reject; FB over-memory → reject; CP spillable over-memory → completes via spill. [quickstart.md](quickstart.md) §§1–2,5.

### Tests for User Story 2 ⚠️

- [ ] T027 [P] [US2] Add unit tests in `crates/compat/src/isolation.rs` for full mapping table + `serializable_nongoal`; SNAPSHOT refuse path when any touched type has `snapshot_capable == false` yields `snapshot_unsupported{type}` naming allowed set (`READ COMMITTED`, `SNAPSHOT`)
- [ ] T028 [P] [US2] Add conformance in `crates/conformance/tests/compat_isolation.rs`: `SERIALIZABLE` → refused (SC-003); CP `REPEATABLE READ` on Relational Table applies Snapshot; same on `K/V Store` → `snapshot_unsupported` with **0** silent downgrades to `READ COMMITTED`
- [ ] T029 [P] [US2] Add conformance in `crates/conformance/tests/compat_admission.rs` (SC-004): drive `query.max_concurrent_per_node`+1 → `admission_rejected` (no hang); FB/HC over `max_memory` with spill off → reject naming `query_memory`; CP spillable sort/hash/agg over memory completes; non-spillable plan over memory → reject naming cap; 0 hangs

### Implementation for User Story 2

- [ ] T030 [P] [US2] Add `TypeDescriptor.snapshot_capable: bool` in `crates/types/` per [research.md](research.md) R5 / FR-006: first-binary defaults Relational Table **true**, Document Store **true**, K/V Store **false**; new types default **false** until descriptor sets true
- [ ] T031 [US2] In `crates/exec/`, attach `IsolationLevel` from `compat::map_sql_isolation` on SQL txn begin; refuse `SERIALIZABLE`; on Snapshot, if any container in the txn has `snapshot_capable == false`, refuse `snapshot_unsupported{type}` — **MUST NOT** silently apply `ReadCommitted` ([isolation.md](contracts/isolation.md))
- [ ] T032 [US2] Map PostgreSQL `SET TRANSACTION ISOLATION LEVEL REPEATABLE READ` → `Snapshot` in `crates/handler-postgresql/`; ClickHouse: no SQL isolation SET in MUST; Cassandra consistency remains `012` (not stored in `IsolationSet`) per FR-007
- [ ] T033 [US2] Ensure FirstBinary/HandlersComplete treat `BEGIN` as MUST NOT (`begin_not_in_profile`) so SNAPSHOT is not required there; document in `crates/compat/src/isolation.rs`
- [ ] T034 [US2] Wire admission acquire order in `crates/exec/` per [data-model.md](data-model.md) §6: node concurrent slot → namespace concurrent slot → memory reservation; failure → `admission_rejected{limit,current,max}`; never hang; spill MUST NOT satisfy a concurrent-query miss (FR-010)
- [ ] T035 [US2] Revise complete-product default `query.spill` to `on` in `crates/config` / `crates/exec` starters when `DialectProfile::CompleteProduct`; mark sort, hash-join build, and aggregation plans spillable; non-spillable (point get, insert) reject naming `query_memory`; operator MAY set `spill off` on CP → all over-memory reject ([research.md](research.md) R4)
- [ ] T036 [US2] Confirm client disconnect / explicit cancel stops work within the `002`/`005` timeout window (FR-009) — wire any missing cancel path in `crates/handler-postgresql/` CancelRequest and `crates/exec/` cancellation token; no new timeout inventing in `compat`
- [ ] T037 [US2] Reuse/increment `query_admission_rejected_total` (existing `005`/`008` labels) on admission rejects; do not rename series ([metrics.md](contracts/metrics.md))

**Checkpoint**: SC-003 and SC-004 admission/isolation paths green for FB (reject) and CP (spill + refuse). No SERIALIZABLE; no silent SNAPSHOT downgrade.

---

## Phase 5: User Story 3 - Size limits and rolling upgrade (Priority: P2)

**Goal**: Documented size/connection maxima with hard reject (first binary **and** complete product). Complete-product-only mixed N/N+1; N refuses N+1 types/formats; N+2 join refused. First binary: every node same product version; mixed fixtures not required for slices 1–5.

**Independent Test**: Oversized value → `limit_exceeded`. Complete product: mixed N/N+1 interoperate; N create of N+1 type refused; N+2 join refused. FB: all nodes `product_version 1`. [quickstart.md](quickstart.md) §§2,3,6.

### Tests for User Story 3 ⚠️

- [ ] T038 [P] [US3] Add unit tests in `crates/compat/src/limits.rs` for `check_size` / connection counters: over `max_value` → `limit_exceeded{limit:"value",…}`; over `max_key` / `max_query_text` / `max_result` named correctly
- [ ] T039 [P] [US3] Add conformance in `crates/conformance/tests/compat_limits.rs` (SC-004 size/connection): write value &gt; `max_value` (default 16MiB) → named reject; open `max_connections_per_entrypoint`+1 → connection refuse (PG `53300` / Redis `-ERR max connections` / HTTP 429|503); over `max_connections_per_principal` after AUTH → refuse; 0 hangs; optional [limits-raised.conf](contracts/fixtures/limits-raised.conf) raises caps and allows the previously oversized write
- [ ] T040 [P] [US3] Add conformance in `crates/conformance/tests/compat_version_window.rs` (SC-005): FB three-node starter — every node reports `product_version 1`, no mixed N/N+1 fixture required; CP (when format versions exist): N/N+1 interoperate; N create of type with `introduced_in > N` → `type_too_new`; N write format N+1 → `format_too_new`; N+2 join → `product_version_window`

### Implementation for User Story 3

- [ ] T041 [US3] Enforce size limits at parse/accept in handlers (`crates/handler-*/`) and result production in `crates/exec/`: key/path/object key ≤ `max_key`; value/document/assembled S3 object (multipart **sum**) ≤ `max_value`; statement/CQL/ES body/CH query ≤ `max_query_text`; streaming result ≤ `max_result` — reject with `limit_exceeded{limit,current,max}`; never truncate silently ([limits.md](contracts/limits.md))
- [ ] T042 [US3] Enforce connection caps in `crates/node/` accept path: `max_connections_per_entrypoint` counts TCP sessions on that listener (including AUTH in progress); `max_connections_per_principal` counts authenticated sessions for `PrincipalId` (`014`) across handlers; exceed → protocol connection error, **do not wait** ([research.md](research.md) R11); `001` buffer-full remains separate `admission_rejected{limit:"buffer"}`
- [ ] T043 [US3] Maintain process-local session counters in `crates/node/` (or `crates/compat` helper): `connections_on_entrypoint`, `connections_of_principal`, `inflight_queries_node`, `inflight_queries_namespace`, `reserved_query_memory` per [data-model.md](data-model.md) §8 — not Raft; lost on restart
- [ ] T044 [P] [US3] Increment `spacestorage_limit_rejected_total{limit,protocol,namespace}` on size/connection rejects; optional gauge `spacestorage_limit_usage_ratio{limit}` at ≥ 0.8 MAY exist — not a substitute for hard reject at 100% ([metrics.md](contracts/metrics.md))
- [ ] T045 [US3] Add `TypeDescriptor.introduced_in: ProductVersion` (default **1**) in `crates/types/`; N node create when `introduced_in > local` → `type_too_new{type,introduced_in,local}`; catalog-diff listing stays `003` / `spacestorage catalog diff --from N --to N+1` ([research.md](research.md) R13)
- [ ] T046 [US3] Import `ProductVersion::peers_ok` in `crates/internode/` join/handshake: peer outside N/N+1 → `product_version_window{local,peer}`; first-binary conformance MUST NOT require two majors in one cluster ([version-window.md](contracts/version-window.md))
- [ ] T047 [US3] Import format-window refuse in `crates/storage/` / `013` path: N node MUST NOT write disk format N+1 → `format_too_new{have,need}`; product N writes format N; N+1 reads N and N+1 ([research.md](research.md) R8)
- [ ] T048 [P] [US3] Expose `GET /v1/limits` and `spacestorage limits` (current limits + usage ratios) token-protected via `014` in `crates/admin-proto` / `crates/spacestorage` CLI per [config-directives.md](contracts/config-directives.md)

**Checkpoint**: SC-004 size/connection green on FB. SC-005 same-version green on FB; mixed N/N+1 refuse edges green on CP when format versions exist.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Operator path, docs alignment, naming discipline, quickstart gate

- [ ] T049 [P] Align [quickstart.md](quickstart.md) and `docs/examples/compat/` so validate → first-binary MUST/MUST NOT → limits reject → same-version cluster can be run under 60 minutes for FB path (SC-001–SC-005 FB)
- [ ] T050 Ensure `crates/compat/Cargo.toml` package description, `crates/compat/README.md`, and any feature changelog under `docs/` never brand the seven-protocol matrix as “v1” (FR-004 / clarify Q1) — prefer `compatibility-ceiling` / `DialectProfile::CompleteProduct`
- [ ] T051 [P] Document in `crates/compat/README.md` that unmodified **applications** needing MUST NOT verbs are out of scope; stock **clients** on MUST verbs are in scope (FR-005)
- [ ] T052 Run full [quickstart.md](quickstart.md) steps 0–6 against in-process/loopback and record the command set under `crates/conformance` so SC-001–SC-005 stay executable
- [ ] T053 [P] `rustfmt` / `clippy` clean pass on `crates/compat` and new conformance modules
- [ ] T054 Confirm CPU hard isolation is **not** claimed in docs/metrics (FR-012); quota **units** remain `007`

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Setup — **BLOCKS** all user stories
- **User Stories (Phase 3–5)**: All depend on Foundational
  - US1 (matrix + handler classify) can proceed without SNAPSHOT/spill
  - US2 (isolation + admission) needs `snapshot_capable` on types and exec admission seams
  - US3 (size + version window) needs Limits defaults + counters; mixed-version needs `012`/`013` format versions (CP only)
- **Polish (Phase 6)**: Depends on desired stories for quickstart; T050/T051/T054 can start after Foundational docs exist

### User Story Dependencies

- **User Story 1 (P1) 🎯 MVP**: After Foundational — matrix + classify wiring. Independently testable via first-binary MUST/MUST NOT conformance
- **User Story 2 (P1)**: After Foundational — may share handler txn paths with US1 CP cursors/BEGIN but isolation/admission independently testable
- **User Story 3 (P2)**: After Foundational — size/connection independent of matrix verbs; rolling-upgrade tests require CP + format versions

### Within Each User Story

- Tests MUST be written and FAIL before implementation
- Compat tables/types before handler/exec wiring
- FB paths before HC/CP feature-gated suites
- Story complete before treating the next as done (stories may still be *staffed* in parallel)

### Parallel Opportunities

- T003 after T001; T005–T010 after T004 skeleton (different files)
- T014–T017 all `[P]` once Foundational is done
- T018–T021 matrix encodings in parallel (different modules/files)
- T027–T029 and T038–T040 in parallel with US1 tests after Foundational
- T022 and T041 can proceed in parallel once Limits + classify APIs exist (different concerns)
- US2 and US3 FB subsets can be staffed while US1 HC/CP suites wait on remaining handlers

---

## Parallel Example: User Story 1

```bash
# Launch all US1 tests together (must fail until matrix + wiring exist):
Task: "T014 matrix unit classify FB"
Task: "T015 compat_first_binary MUST/MUST NOT"
Task: "T016 compat_handlers_complete"
Task: "T017 compat_complete_product"

# Then encodings + wiring:
Task: "T018–T021 matrix / cursors / ES / COPY tables"
Task: "T022–T023 handler classify before IR"
Task: "T024–T026 profile gate, metrics, CP cursors"
```

## Parallel Example: User Stories 2 and 3 (after Foundational)

```bash
Task: "T027–T029 isolation + admission tests"
Task: "T038–T040 limits + version-window tests"
Task: "T030 snapshot_capable on types"
Task: "T041–T043 size/connection enforcement + counters"
Task: "T045–T047 introduced_in + peers_ok + format_too_new"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup (`crates/compat` member)
2. Complete Phase 2: Foundational (CRITICAL — types, defaults, errors, config parse)
3. Complete Phase 3: User Story 1 (matrix + first-binary PG/Redis classify + conformance)
4. **STOP and VALIDATE**: `cargo test -p spacestorage-compat` and first-binary MUST/MUST NOT suite
5. Demo [quickstart.md](quickstart.md) §1

**Note**: Shipping a usable first binary also needs US2 admission reject + US3 size/connection reject (FB subsets). Treat those as the immediate follow-on after US1 MVP, still before HC/CP and mixed-version work.

### Incremental Delivery

1. Setup + Foundational → `spacestorage-compat` types exist
2. US1 FB matrix → independently demoable ceiling for PG/Redis
3. US2 FB admission/memory reject + US3 size/connection → SC-004 FB
4. US1 HC (slice 6) → remaining handlers at matrix
5. US2 CP SNAPSHOT/spill + US1 CP cursors/COPY/ES aggs (slice 8)
6. US3 CP mixed N/N+1 refuse edges → SC-005 CP
7. Polish: quickstart, naming, clippy

### Parallel Team Strategy

1. Team completes Setup + Foundational together
2. Developer A: US1 matrix + handler classify + conformance
3. Developer B: US2 isolation + admission/spill + `snapshot_capable`
4. Developer C: US3 Limits enforcement + version window + admin `limits` CLI

---

## Notes

- [P] tasks = different files, no dependencies on incomplete tasks
- [US1]/[US2]/[US3] map to spec stories (US1+US2 are P1; US1 is the matrix MVP)
- Do not reimplement `002` handlers or `005` planner internals — consume `compat` as the single table
- Do not brand the matrix “v1”; first binary is a **subset** via `DialectProfile::FirstBinary`
- Mixed N/N+1 and spill are complete-product; FB same-version + hard-reject only
- Verify tests fail before implementing
- Stop at any checkpoint to validate a story independently
- Avoid: vague tasks, stuffing matrix into `protocol-core`, silent SNAPSHOT downgrade, spill as concurrent-query substitute

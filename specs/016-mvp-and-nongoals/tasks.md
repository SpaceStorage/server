---
description: "Task list for MVP cut, sequencing, and product non-goals"
---

# Tasks: MVP Cut, Sequencing, and Product Non-Goals

**Input**: Design documents from `/specs/016-mvp-and-nongoals/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md), [data-model.md](data-model.md), [contracts/](contracts/)

**Tests**: Requested. Spec Independent Tests + SC-001–SC-005 + [conformance-profile.md](contracts/conformance-profile.md) (`cargo test -p spacestorage-conformance --features first-binary`, `cargo test -p spacestorage-release-profile --test ledger`, `cargo test -p spacestorage-release-profile --test nongoals`). Write failing tests first.

**Scope of this feature**: `016` ships a **release profile**, **slice ledger**, **starter configs**, and **conformance suite**. Protocol, type, WAL, membership, and quorum **behavior** stay in `001`–`015`. This feature **selects and proves** the slices 1–5 subset. Do not reimplement handlers or invent a second query engine.

**Sibling crates** (from `001`–`015` plans; create the seam, do not duplicate internals): `crates/config`, `crates/node`, `crates/spacestoraged`, `crates/spacestorage`, `crates/handler-postgresql`, `crates/handler-redis`, `crates/conformance`.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: [US1], [US2], [US3] on user-story phase tasks only
- Every task includes an exact file path

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Workspace member, crate skeleton, docs trees, Cargo feature inventory from [release-profile.md](contracts/release-profile.md)

- [ ] T001 Create `crates/release-profile/Cargo.toml` (package `spacestorage-release-profile`, edition 2024) and `crates/release-profile/src/lib.rs` that `mod`s `slice`, `profile`, `handlers`, `types`, `nongoals`, `ledger`
- [ ] T002 Add `crates/release-profile` to workspace `[workspace.members]` in `Cargo.toml` and encode `[features] default = ["first-binary"]` with `first-binary = ["handler-postgresql", "handler-redis"]` and `complete-product` pulling `handler-cassandra`, `handler-elasticsearch`, `handler-clickhouse`, `handler-s3`, `handler-webdav` exactly as [release-profile.md](contracts/release-profile.md)
- [ ] T003 [P] Create `docs/milestones/README.md` (how to record a slice milestone; tags MAY be `slices-1-5` not `v1.0.0`) and `docs/milestones/000-template.md` requiring a `## Deferred` heading per [milestone-record.md](contracts/milestone-record.md)
- [ ] T004 [P] Copy [contracts/fixtures/first-binary-one-node.conf](contracts/fixtures/first-binary-one-node.conf) to `docs/examples/first-binary-one-node.conf` and the three-node set to `docs/examples/first-binary-three-node/{a,b,c}.conf`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: In-crate types and validation codes every story uses. No user-story work until this phase is complete.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [ ] T005 Implement `SliceId` in `crates/release-profile/src/slice.rs` as `Runtime=1` … `UisIngest=11` with field rules `id` `1..=11, unique, stable forever`; `intent_files` non-empty list of `01`–`15`; `first_binary` iff `id <= 5`; constants `FIRST_BINARY` = `1..=5` and `DEFERRED_AFTER_FIRST` = `{6,7,8,9,10,11}`; `skip_policy`: a milestone MAY omit `id > implemented_max` only as `DeferredSlice`, never by deleting intent
- [ ] T006 [P] Implement `ReleaseProfile { FirstBinary, CompleteProduct }` in `crates/release-profile/src/profile.rs` and `HandlerBuildSet { required, forbidden }` in `crates/release-profile/src/handlers.rs` where FirstBinary required = `admin`, `admin-http`, `internode`, `replication`, `postgresql`, `redis` and forbidden = `cassandra`, `elasticsearch`, `clickhouse`, `clickhouse-http`, `s3`, `webdav`; `syslog` is not required in either profile until slice 11
- [ ] T007 [P] Implement `TypeRequirement` in `crates/release-profile/src/types.rs` with `required_creatable_l3` exactly `K/V Store`, `Relational Table`, `Document Store`
- [ ] T008 [P] Implement `ProductNonGoal` in `crates/release-profile/src/nongoals.rs` with ids `SecondQueryEnginePerProtocol`, `DropInReplacementOfEmulatedSystems`, `KafkaAsStoredLogProduct`, `HumanPickedMultiMasterConflict`, `ByzantineNodes`, `PerTenantCpuHardIsolation`, `NativeClientProtocol`, `SqlSerializable`, `MultiActiveOnInFirstBinary` mapped to owning spec paths from [non-goals.md](contracts/non-goals.md)
- [ ] T009 Implement `DeferredSlice { id, reason, still_owed }` and `MilestoneRecord { slug, implemented, deferred, profile, changelog_ref }` in `crates/release-profile/src/ledger.rs` with YAML parse; `reason` non-empty; `still_owed` MUST be true unless the slice’s entire content is a `ProductNonGoal` (none of 6–11 are)
- [ ] T010 Export validation codes in `crates/release-profile/src/lib.rs` (or `error.rs`): `slice_unknown`, `slice_gap` (implemented set must be a prefix `1..=k`; no holes), `deferred_missing`, `deferred_marked_cancelled`, `implemented_not_prefix`, `profile_mismatch`, `profile_incomplete`, `nongoal_unspecified{id, spec}` plus named reuse codes `entrypoint_unknown_handler`, `transport_undeclared`, `internode_required`, `replication_required`, `master_key_required`, `master_key_unreadable`, `topology_ladder_required`, `multi_active_unsupported`, `feature_not_supported`
- [ ] T011 Add unit tests in `crates/release-profile/src/slice.rs` and `crates/release-profile/src/ledger.rs` covering `slice_gap` on holes, `deferred_missing` when `k==5` and deferred ≠ `{6,7,8,9,10,11}`, `profile_mismatch` when `FirstBinary` is not `k==5`, `profile_incomplete` when `CompleteProduct` is not `1..=11`

**Checkpoint**: `cargo test -p spacestorage-release-profile` compiles and prefix/ledger unit tests pass. User stories can start.

---

## Phase 3: User Story 1 - First shippable binary (Priority: P1) 🎯 MVP

**Goal**: Encode and prove slices 1–5 DoD: 1-node and 3-node starters with ladder `[az]`; types KV / Relational Table / Document Store; PostgreSQL smoke without COPY/BEGIN; Redis MUST list on `K/V Store` only; write TWO / read ONE durable WAL acks; UUID + join secret; always-on `internode`/`replication`; explicit `tls` or `plaintext;`; admin CLI/HTTP + drain; `/metrics`; master-key file; `multi_active=on` create refused; Cassandra/ES/CH/S3/WebDAV absent (`entrypoint_unknown_handler`). Not a “v1” of the seven-protocol matrix.

**Independent Test**: Bring up 1-node then 3-node from starter examples; PG and Redis smokes; kill one node; write at TWO; restart; confirm restore. `cargo test -p spacestorage-conformance --features first-binary`.

### Tests for User Story 1 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T012 [P] [US1] Add G1/G2 tests in `crates/conformance/tests/first_binary.rs`: validate+start [first-binary-one-node.conf](contracts/fixtures/first-binary-one-node.conf) → `ready`, `internode`+`replication` bound on loopback, `/metrics` scrapeable; omitted-transport fixture fails; bootstrap A and join B/C from three-node fixtures → membership identical, ladder `[az]` with three values
- [ ] T013 [P] [US1] Add G3/G4 tests in `crates/conformance/tests/dialect_pg.rs`: `tokio-postgres` CREATE/INSERT/SELECT/UPDATE/DELETE/DROP simple+extended auto-commit succeed; `COPY` and `BEGIN` return PostgreSQL `ERRCODE_FEATURE_NOT_SUPPORTED` (`0A000`) with no data change
- [ ] T014 [P] [US1] Add G5 tests in `crates/conformance/tests/dialect_redis.rs`: AUTH, PING, GET, SET, DEL, EXISTS, SCAN, SELECT as no-op, TTL/EXPIRE/PTTL mapped to container TTL on a `K/V Store` only
- [ ] T015 [P] [US1] Add G8 tests in `crates/conformance/tests/handlers_absent.rs`: [unknown-handler-cassandra.conf](contracts/fixtures/invalid/unknown-handler-cassandra.conf) → `entrypoint_unknown_handler{handler=cassandra, known=[admin, admin-http, internode, postgresql, redis, replication]}`; elasticsearch/clickhouse/s3/webdav named in config fail the same way
- [ ] T016 [P] [US1] Add G9 tests in `crates/conformance/tests/multi_active.rs`: catalog create with `multi_active=on` → `multi_active_unsupported`; flag default off
- [ ] T017 [P] [US1] Add G6/G7 tests in `crates/conformance/tests/first_binary.rs` (or `quorum_restore.rs`): default write TWO with one node killed succeeds (two durable WAL acks in source `quorum_domain`); kill a second node → TWO fails; restart killed node → previous TWO write readable
- [ ] T018 [P] [US1] Add G10 drain test in `crates/conformance/tests/first_binary.rs`: drain one member → no new tenant connections; in-flight bound by drain timeout
- [ ] T019 [P] [US1] Add config unit tests in `crates/config/src/validate.rs` (or `crates/config/tests/first_binary.rs`) that [omitted-transport.conf](contracts/fixtures/invalid/omitted-transport.conf) yields `transport_undeclared` and a missing `internode`/`replication` entrypoint yields `internode_required` / `replication_required`

### Implementation for User Story 1

- [ ] T020 [US1] Register only `ReleaseProfile::FirstBinary` handlers in `crates/node/src/handler/mod.rs` via Cargo features; default `cargo build -p spacestoraged` MUST NOT link cassandra/elasticsearch/clickhouse/s3/webdav
- [ ] T021 [US1] In `crates/config/src/validate.rs` and `crates/node` startup, a config naming a handler not in `HandlerRegistry` fails collect-all `entrypoint_unknown_handler{handler, known}` (`001` reuse)
- [ ] T022 [US1] Reject every entrypoint missing `tls {…}` and `plaintext;` with `transport_undeclared` in `crates/config/src/validate.rs` (`014` reuse)
- [ ] T023 [US1] Require `internode` and `replication` always listening for this profile (default bind `127.0.0.1`) via `internode_required` / `replication_required` in `crates/config/src/validate.rs`; join of remotes still requires a cluster address (`012`)
- [ ] T024 [US1] Require `cluster { master_key_file <path>; }` with mode ≤ 0600 (`master_key_required` / `master_key_unreadable`) in `crates/config/src/validate.rs` and `crates/node` startup; no required external KMS
- [ ] T025 [US1] Default missing `topology_ladder` to `[az]` at resolve time (`topology_ladder_required` if still missing after resolve) in `crates/config/src/resolve.rs`; starter fixtures already declare `topology_ladder az` and `quorum_domain lab`
- [ ] T026 [US1] Refuse container create with `multi_active=on` as `multi_active_unsupported` in the catalog create path (`crates/node` / types crate from `003`/`012`); catalog flag remains, default off; create never leaves `creating`
- [ ] T027 [US1] Select `DialectProfile::FirstBinary` for PostgreSQL in `crates/handler-postgresql` from `spacestorage-release-profile`: wire 3.0, SCRAM, simple+extended, INSERT/SELECT/UPDATE/DELETE, CREATE/DROP table, auto-commit; `BEGIN`/`COMMIT`/`ROLLBACK`/`COPY` → `feature_not_supported` `0A000` (recommended: not-supported for all three txn verbs so the dialect is one rule)
- [ ] T028 [US1] Select first-binary Redis dialect in `crates/handler-redis`: AUTH/PING/GET/SET/DEL/EXISTS/SCAN/SELECT-noop/TTL on `K/V Store`; other types canonical blob only; type-specific verbs off KV not required
- [ ] T029 [US1] Include compile-time `"release_profile": "first-binary"` and registered `handlers` in `spacestorage status --output json` and admin `GET /v1/status` (`crates/admin-proto` + `crates/node/src/admin`); there is no config knob that enables Cassandra without rebuilding with `complete-product`
- [ ] T030 [US1] Expose global `/metrics` for implemented paths only (runtime/buffer/`node_state`, postgresql/redis traffic, WAL and replication ack counters used by TWO) via the `008` exposition seam; do not emit placeholder series for unimplemented families
- [ ] T031 [US1] Ensure node `ready` for this profile additionally requires: both cluster handlers bound, master-key readable, every entrypoint has transport, membership path bootstrap or joined (`crates/node/src/lifecycle.rs`)

**Checkpoint**: `cargo test -p spacestorage-conformance --features first-binary` passes G1–G10 against in-process nodes. `spacestorage status --output json` reports `"first-binary"`. Unknown-handler and omitted-transport fixtures fail as specified. Do not brand this artifact as seven-protocol “v1”.

---

## Phase 4: User Story 2 - Sequencing without deleting intent (Priority: P1)

**Goal**: Implementation follows slices 1→11; a 1–5 milestone lists slices 6–11 as **deferred** (`still_owed: true`), never deleted from `01`–`15`. Slice 6, when done, speaks the `015` MUST subset — not a private smaller list.

**Independent Test**: Planning checklist maps each slice to intent files; confirm 6–11 still listed as owed. `cargo test -p spacestorage-release-profile --test ledger`.

### Tests for User Story 2 ⚠️

- [ ] T032 [P] [US2] Add `crates/release-profile/tests/ledger.rs` that reads `docs/milestones/*.yaml`: any record with `implemented: [1,2,3,4,5]` MUST have deferred ids `6,7,8,9,10,11` and `still_owed: true` on each; `still_owed: false` → `deferred_marked_cancelled`; holes → `slice_gap`; `profile: complete-product` with missing slices → `profile_incomplete`
- [ ] T033 [P] [US2] Add a fixture `docs/milestones/fixtures/missing-deferred.yaml` that omits slice 8 and assert the ledger test fails with `deferred_missing`
- [ ] T034 [P] [US2] Add `scripts/check-milestone.sh` wrapping `cargo test -p spacestorage-release-profile --test ledger` so a 1–5 changelog without deferred 6–11 fails CI (SC-004)

### Implementation for User Story 2

- [ ] T035 [P] [US2] Encode slice→intent map from [slices.md](contracts/slices.md) as `SliceId::intent_files()` in `crates/release-profile/src/slice.rs` (1→`001`; 2→`003`,`013`,`014` master-key subset; 3→`002` postgresql; 4→`002` redis + `015` KV MUST; 5→`011`,`012`,`004` quorum; 6→ remaining handlers at `015` MUST; 7→`006`,`007`,`014`; 8→`005`; 9→`008`; 10→`010`,`013`; 11→`009`)
- [ ] T036 [US2] Parse `docs/milestones/<nnn>-<slug>.yaml` in `crates/release-profile/src/ledger.rs` to the schema in [milestone-record.md](contracts/milestone-record.md) (`slug`, `profile: first-binary|complete-product`, `implemented`, `deferred: [{id, still_owed, reason}]`, `changelog_ref`); markdown sibling MUST contain `## Deferred` listing the same ids
- [ ] T037 [US2] Add `docs/milestones/001-first-binary.yaml` and `docs/milestones/001-first-binary.md` for the slices 1–5 ship: `profile: first-binary`, `implemented: [1,2,3,4,5]`, deferred 6–11 each `still_owed: true` with reasons from the contract example; `changelog_ref: docs/milestones/001-first-binary.md`
- [ ] T038 [US2] Document in `docs/milestones/README.md` that skipping a later slice in an implementation milestone is `DeferredSlice`, not deletion of `.specify/intent/01`–`15`; slice 6 gate is the **`015` complete-product MUST column**, not [dialect-first-binary.md](contracts/dialect-first-binary.md)

**Checkpoint**: `cargo test -p spacestorage-release-profile --test ledger` passes on `001-first-binary.yaml` and fails on the missing-deferred fixture. Intent files `01`–`15` are untouched.

---

## Phase 5: User Story 3 - Product non-goals stay out of specs (Priority: P1)

**Goal**: Product non-goals are Out of Scope or a named refuse in owning specs — not “later”, not implied by type/protocol inventories. Later-not-first items stay in deferred slices, not this list.

**Independent Test**: Review specs `01`–`15` Out of Scope / non-goals; none of the non-goal list is a MUST. `cargo test -p spacestorage-release-profile --test nongoals`.

### Tests for User Story 3 ⚠️

- [ ] T039 [P] [US3] Add `crates/release-profile/tests/nongoals.rs` that greps owning spec paths from [non-goals.md](contracts/non-goals.md) for each `ProductNonGoal` id (or its statement); missing mention → `nongoal_unspecified{id, spec}`
- [ ] T040 [P] [US3] Add a negative fixture (test-only string or temp file) proving the audit fails when `SqlSerializable` is absent from a cited spec path

### Implementation for User Story 3

- [ ] T041 [P] [US3] Complete `crates/release-profile/src/nongoals.rs` `REQUIRED_MENTIONS: &[(ProductNonGoal, &[&str])]` with paths: second query engine → `specs/002-protocol-drivers/spec.md`, `specs/005-query-execution/spec.md`; drop-in replacement → `specs/015-compatibility-and-limits/spec.md`; Kafka as stored log → `specs/003-type-system/spec.md`, `specs/008-observability/spec.md`, `specs/009-admin-ui-ingest/spec.md`; human conflict / Byzantine → `specs/012-internode-and-time/spec.md`; CPU hard isolation → `specs/015-compatibility-and-limits/spec.md`, `specs/007-tenancy-security/spec.md`; native client protocol → `.specify/memory/constitution.md`, `specs/002-protocol-drivers/spec.md`; SERIALIZABLE → `specs/015-compatibility-and-limits/spec.md`, `specs/005-query-execution/spec.md`; `multi_active=on` → `specs/012-internode-and-time/spec.md`, `specs/016-mvp-and-nongoals/spec.md`
- [ ] T042 [US3] Confirm (and if a cited spec is silent, add an Out of Scope bullet **only** in that spec’s Out of Scope section — do not rewrite `01`–`15` interiors) that Kafka is ingest (`009`) + outbound logging (`008`) with Log Stream as the type; `SERIALIZABLE` and a second query engine are refused; no `handler spacestorage` in inventories
- [ ] T043 [US3] Keep later-not-first items (UIs/ingest, full L0 creatable-as-workflow, planetary production examples, external KMS, mixed-version upgrade, federated/union at planetary scale, billing formula, GDPR workflows, CDC beyond Log Stream + WAL) **out** of `ProductNonGoal` and **in** deferred slices / `01`–`15` as [non-goals.md](contracts/non-goals.md) “Later, not first binary”

**Checkpoint**: `cargo test -p spacestorage-release-profile --test nongoals` passes. Non-goals are not silent backlog; later-not-first is not classified as non-goals.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Operator path, CI isolation, naming discipline

- [ ] T044 [P] Align `docs/examples/` and [quickstart.md](quickstart.md) ports/paths (`5432`–`5434`, `6379`–`6381`, `7700`–`7703`, `7800`–`7803`, `7900`–`7903`) so an operator can finish 1-node then 3-node + PG/Redis smokes in under 60 minutes (SC-001)
- [ ] T045 [P] Document in `docs/milestones/README.md` that default CI is `--features first-binary` (or default features) and a `complete-product` job MUST NOT be a merge gate for slices 1–5
- [ ] T046 Ensure no published artifact, changelog title, or crate description calls the first binary a “v1” of the seven-protocol matrix (FR-002) — prefer `slices-1-5` / `first-binary`
- [ ] T047 Run [quickstart.md](quickstart.md) steps 2, 3, 4 (`BEGIN`/`COPY`), 5, 8 against the in-process or loopback binary and record the command set in `crates/conformance` so SC-001–SC-003 stay executable
- [ ] T048 [P] Add `rustfmt`/`clippy` clean pass on `crates/release-profile` and new conformance tests

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately (workspace member + docs trees)
- **Foundational (Phase 2)**: Depends on Setup — **BLOCKS** all user stories
- **User Stories (Phase 3–5)**: All depend on Foundational
  - US1 (first binary wiring + conformance) needs sibling crates from `001`–`005`/`011`–`014` for G1–G10 to go green; profile types do not
  - US2 (ledger) is independent of a running node once T009 exists
  - US3 (nongoal audit) is independent of a running node once T008 exists
- **Polish (Phase 6)**: Depends on US1–US3 as needed for quickstart; T045/T046 can start after US2 docs exist

### User Story Dependencies

- **User Story 1 (P1) 🎯 MVP**: After Foundational. Uses `HandlerBuildSet` / `TypeRequirement` / dialect selection. Independent Test = first-binary conformance.
- **User Story 2 (P1)**: After Foundational. Uses `SliceId` / `MilestoneRecord`. Independently testable via `--test ledger` even if US1 node tests are red.
- **User Story 3 (P1)**: After Foundational. Uses `ProductNonGoal`. Independently testable via `--test nongoals`.

### Within Each User Story

- Tests MUST be written and FAIL before implementation
- Profile types (Phase 2) before node/config wiring
- Config validation before handler dialect selection
- Story complete before treating the next as done (stories may still be *staffed* in parallel)

### Parallel Opportunities

- T003, T004 after T001
- T006, T007, T008 after T005 starts (different files)
- T012–T019 all `[P]` once Foundational is done
- T032–T034 and T039–T040 in parallel with US1 tests
- T035 and T041 in parallel (different files)
- US2 and US3 can be fully implemented while US1 waits on sibling `001`–`015` crates

---

## Parallel Example: User Story 1

```bash
# Launch all US1 tests together (must fail until wiring exists):
Task: "G1/G2 first_binary.rs"
Task: "G3/G4 dialect_pg.rs"
Task: "G5 dialect_redis.rs"
Task: "G8 handlers_absent.rs"
Task: "G9 multi_active.rs"
Task: "G6/G7 quorum restore"
Task: "G10 drain"
Task: "config omitted-transport / cluster-ports"

# Then wiring (order: registry → validate → dialects → status/metrics):
Task: "T020 handler feature registration"
Task: "T021–T025 config/node profile checks"
Task: "T026–T028 multi_active + PG/Redis dialects"
Task: "T029–T031 status, metrics, ready"
```

## Parallel Example: User Stories 2 and 3 (no running node)

```bash
Task: "T032–T034 ledger tests + check-milestone.sh"
Task: "T039–T040 nongoals tests"
Task: "T035 slice intent map"
Task: "T041 REQUIRED_MENTIONS"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL)
3. Complete Phase 3: User Story 1 (conformance G1–G10 + profile wiring)
4. **STOP and VALIDATE**: `cargo test -p spacestorage-conformance --features first-binary`
5. Demo the [quickstart.md](quickstart.md) 1-node path

### Incremental Delivery

1. Setup + Foundational → `release-profile` types and ledger rules exist
2. US3 nongoal audit (docs invariant, no daemon)
3. US2 first-binary milestone record with deferred 6–11
4. US1 first-binary binary + conformance (the shippable artifact)
5. Polish: CI isolation, naming, quickstart timing

### Parallel Team Strategy

1. Team completes Setup + Foundational together
2. Developer A: US1 (node/config/handlers/conformance)
3. Developer B: US2 (ledger + milestone files)
4. Developer C: US3 (nongoal grep audit + Out of Scope bullets if a cited spec is silent)

---

## Notes

- [P] tasks = different files, no dependencies on incomplete tasks
- [US1]/[US2]/[US3] map to spec stories (all P1; US1 is the first-binary MVP)
- Do not implement Cassandra/ES/CH/S3/WebDAV, Raft, MapReduce, UIs, or Kafka ingest in this feature — those are deferred slices 6–11
- Do not treat product non-goals as deferred slices
- Verify tests fail before implementing
- Stop at any checkpoint to validate a story independently

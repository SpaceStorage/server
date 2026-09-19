---
description: "Task list for data migration, transforms, and backup/restore jobs"
---

# Tasks: Data Migration and Type/Model Transforms

**Input**: Design documents from `/specs/010-migration-transforms/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md), [data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: Requested. Plan Testing section + Independent Tests (US1–US4) + SC-001–SC-006 + `contracts/fixtures/`. Unit: mapping catalog vs query refuse, name collision, quota start/cutover, dual-write sequence, move drop. Conformance feature `migrate`: two- then three-node live migrate; namespace copy/move; five rewrite kinds; live writes through swap; mapping query; missing mapping query; snapshot+restore job; encrypted backup missing key. Contract tests on `contracts/fixtures/`. Write failing tests first where listed.

**Scope of this feature**: Slice 10 (`016`) — one library crate `crates/migrate` (`spacestorage-migrate`) owning four `08` job names (`data_migration`, `data_transformation`, `data_backup`, `data_restore`). Dual-write interceptor + catch-up; transform rewrites into a new container; backup/restore **orchestrate** `013` only. First binary stubs with `MigrateSlice10Required`. Replica streaming/repair stays `04`.

**Sibling crates** (seams only; do not reimplement internals): `crates/types` (`003` catalog/`incomplete`), `crates/exec` (`005` `LogicalRequest`), `crates/placement` (`004` cutover validate), `crates/storage` (`013` SnapshotApi), `crates/controlplane` (`006` JobRecord), `crates/node` (interceptor + supervisor), `crates/admin-proto` / `crates/spacestorage` (CLI + JSON), `crates/release-profile` (first-binary stub vs slice 10), `crates/conformance` (feature `migrate`).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: [US1], [US2], [US3], [US4] on user-story phase tasks only
- Every task includes an exact file path

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Workspace member and crate skeleton per [plan.md](plan.md) Project Structure

- [ ] T001 Create `crates/migrate/Cargo.toml` (package `spacestorage-migrate`, edition 2024, MSRV-compatible with workspace) depending only on existing workspace crates (`tokio`, `serde`/`serde_json`, `uuid`, `bytes`, `tracing`, `async-trait`, `parking_lot`, plus seams for types/exec/placement/storage/controlplane as they exist)
- [ ] T002 Create `crates/migrate/src/lib.rs` that `mod`s `job`, `store`, `authz`, `quota`, `dual_write`, `migrate`, `transform`, `mapping`, `backup`, `error` and exports `JobService` (`start` / `cancel` / `status` / `list` / `resume`)
- [ ] T003 Add `crates/migrate` to workspace `[workspace.members]` in `Cargo.toml` and wire optional compile on slice 10 via `crates/release-profile` / Cargo features so first-binary builds do not require live migrate

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Shared job model, persistence, authz, quota, dual-write window, errors, config, metrics, and first-binary refuse — MUST complete before any user story

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [ ] T004 Implement `JobId` (UUID v7), `JobKind` (`DataMigration`/`DataTransformation`/`DataBackup`/`DataRestore` with wire labels `data_migration`/`data_transformation`/`data_backup`/`data_restore`), `JobStatus` (`starting`→`running`→`completed`|`failed`), `paused: bool` (not a fifth `08` status), `Strategy` (`live` default, `snapshot`, `offline`), and `JobProgress` (`bytes_copied`, `objects_copied`, optional `bytes_remaining`/`objects_remaining`/`install_seq`/`last_applied_source_seq`/`last_source_seq`) in `crates/migrate/src/job.rs` per [data-model.md](data-model.md) §§1–4 and [contracts/jobs.md](contracts/jobs.md)
- [ ] T005 [P] Implement named errors in `crates/migrate/src/error.rs`: `MigrateSlice10Required`, `NameExists { container }`, `QuotaExceeded`, `ConstraintUnsatisfiable`, `MappingQueryRequired`, `MappingSourceMissing { name }`, `JobInProgress`, `AuthzDenied`, `StateUnreadable`, `KeyRefMissing`, `NoOp`, `NotSupported { what }` per [contracts/jobs.md](contracts/jobs.md)
- [ ] T006 Implement `JobStore` in `crates/migrate/src/store.rs`: persist `JobRecord` via `006` `ClusterStore.append`; **cluster log** when job names two namespaces, node/drive placement, or backup scope wider than one namespace; **namespace log** when source and dest share one namespace; resume any `ready` node from `last_applied_source_seq` (R3)
- [ ] T007 [P] Implement authz helpers in `crates/migrate/src/authz.rs`: node/drive migrate → `MIGRATE` on container **or** `CLUSTER_ADMIN`; same-namespace transform → `MIGRATE`; cross-namespace copy/move/restore → `CLUSTER_ADMIN` **or** same principal holds `MIGRATE` on **both** namespace ids (AND of two bindings) per R15 / FR-010
- [ ] T008 [P] Implement quota checks in `crates/migrate/src/quota.rs`: at start ask `07` whether dest namespace logical usage + source size (second copy) fits — refuse `QuotaExceeded` before create; before cutover/swap re-read and fail named with source intact and target `incomplete` (no reservation) per R11 / FR-011
- [ ] T009 Implement dual-write window types and interceptor skeleton in `crates/migrate/src/dual_write.rs`: at most one `DualWriteWindow` per source (`JobInProgress` otherwise); fields `container_id`, `job_id`, `install_seq`, `target_id`; catch-up uses container source-log / HLC seq (`012`); idempotent apply (same key+seq no-op) per [contracts/dual-write.md](contracts/dual-write.md) and R4–R5
- [ ] T010 [P] Implement incomplete-target helpers (create with `incomplete: true`, internal name `ss:job:<job_id>`, omit from tenant LIST) and cutover/swap gate checklist in `crates/migrate/src/lib.rs` or `crates/migrate/src/dual_write.rs`: `last_applied_source_seq` ≥ source head; quota re-check; placement re-check (`04`); target complete; public dest name free **or** is swap source name — then unregister interceptor and publish names per [data-model.md](data-model.md) §§11–14
- [ ] T011 [P] Register `jobs { enabled; catchup_concurrency; }` in the `001` config grammar (e.g. `crates/config`) per [contracts/config-directives.md](contracts/config-directives.md); first-binary + `jobs.enabled on` → startup `MigrateSlice10Required`; slice 10 compiled → default `enabled on`
- [ ] T012 [P] Wire `08` job metric increments (existing series only; `job` ∈ `data_*`; paused keeps series `status=running`) in `crates/migrate` (or observability seam) per [contracts/metrics.md](contracts/metrics.md) / R16
- [ ] T013 Stub `JobService` on first-binary profile in `crates/release-profile` / `crates/migrate` so create/list of `data_*` jobs return `MigrateSlice10Required` (501 on HTTP) without running copy (R2)
- [ ] T014 Add unit tests in `crates/migrate/src/job.rs` (or `crates/migrate/tests/job_status.rs`) covering status machine, unknown kind refuse, and `paused` remaining `running` for `08`

**Checkpoint**: `cargo test -p spacestorage-migrate` compiles; job/store/authz/quota/dual-write foundation ready; first-binary stub refuses jobs. User stories can start.

---

## Phase 3: User Story 1 - Migrate a container between nodes or drives (Priority: P1) 🎯 MVP

**Goal**: Operator-initiated node/drive migrate with default **live** dual-write + catch-up + cutover; strategies `snapshot`/`offline` selectable; `04` placement validated before start and cutover; progress queryable; resume/cancel; `MIGRATE`/`CLUSTER_ADMIN`. Automatic RF repair stays `04`.

**Independent Test**: Two-node then three-node: migrate a KV container off node A onto node B; data identical; A holds no residual replica unless RF still places one. Anti-affinity violation → `ConstraintUnsatisfiable`. Progress bytes/objects while `running`. Mid-failure resume from recorded seq without duplicating keys or fail named.

### Tests for User Story 1 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T015 [P] [US1] Add contract/fixture tests in `crates/migrate/tests/fixtures_evacuate.rs` loading [contracts/fixtures/evacuate-live.json](contracts/fixtures/evacuate-live.json) and asserting job shape + refuse paths under `contracts/fixtures/invalid/`
- [ ] T016 [P] [US1] Add unit tests in `crates/migrate/tests/dual_write_sequence.rs` for dual-write ack only after target apply, pause without client ack on target failure, and cutover gate on `last_applied_source_seq`
- [ ] T017 [P] [US1] Add conformance tests behind feature `migrate` in `crates/conformance/tests/migrate_live.rs`: two-node then three-node live KV evacuate; identical logical data; residual replicas match dest policy (SC-001)
- [ ] T018 [P] [US1] Add conformance test in `crates/conformance/tests/migrate_live.rs` that unsatisfiable anti-affinity returns `ConstraintUnsatisfiable` with no copy started
- [ ] T019 [P] [US1] Add conformance test in `crates/conformance/tests/migrate_resume.rs` that killing a node mid-migration resumes at `last_applied_source_seq` without duplicating committed keys or fails `StateUnreadable` (FR-008 / SC-004)

### Implementation for User Story 1

- [ ] T020 [US1] Implement `MigrationSpec` validation in `crates/migrate/src/migrate.rs`: `source` `ContainerRef`; optional `dest_nodes`/`dest_drives`/`replica_slots`; `strategy`; refuse `NoOp` when same ns + copy + no dest nodes/drives; call `PlacementDirector` before start (FR-009 / R7)
- [ ] T021 [US1] Implement live migrate runner in `crates/migrate/src/migrate.rs`: create incomplete target; register interceptor; catch-up `seq <= install_seq`; dual-write mapped writes; cutover when gates pass; unregister interceptor; update `JobProgress` while `running`
- [ ] T022 [P] [US1] Implement `snapshot` strategy path in `crates/migrate/src/migrate.rs` invoking `013` snapshot of source then restore into dest (no second snapshot format)
- [ ] T023 [P] [US1] Implement `offline` strategy in `crates/migrate/src/migrate.rs`: refuse new source writes (`paused_writes`) until copy completes per [contracts/dual-write.md](contracts/dual-write.md)
- [ ] T024 [US1] Wire `job.create`/`cancel`/`resume`/`status` for `data_migration` in `crates/migrate/src/lib.rs` and register interceptor + job supervisor in `crates/node` so any `ready` node can drive the job
- [ ] T025 [US1] Expose `POST /v1/migrate`, `POST /v1/jobs`, `GET /v1/jobs`, `GET /v1/jobs/{id}`, `POST /v1/jobs/{id}/cancel`, `POST /v1/jobs/{id}/resume` in admin-http (`crates/admin-proto` / node admin) per [contracts/admin-http.md](contracts/admin-http.md)
- [ ] T026 [US1] Implement CLI `spacestorage migrate --from … --to-node … [--strategy live|snapshot|offline]` and `spacestorage job {list|status|cancel|resume}` in `crates/spacestorage` per [contracts/cli.md](contracts/cli.md); exit 5 for `MigrateSlice10Required`

**Checkpoint**: Live node evacuate works on loopback; anti-affinity refuse; progress queryable; resume/cancel correct. MVP demoable via [quickstart.md](quickstart.md) §1 (SC-006 < 20 min small dataset).

---

## Phase 4: User Story 2 - Migrate between namespaces (Priority: P1)

**Goal**: Cross-namespace **copy** (default) or **move** (drop source after cutover — name reusable, storage released); refuse existing dest name; quota at start and cutover; dual `MIGRATE` or admin; incomplete targets never tenant-visible.

**Independent Test**: Copy `acme.t` → `beta` as admin; `beta` matches, `acme` still has table. Move same shape; `acme.t` dropped (name reusable), `beta.t` holds data. Principal without `MIGRATE` refused. Existing `beta.t` → `NameExists`. Start over-quota refused before copy; growth past quota before cutover → job failed, source intact, dest not tenant-visible.

### Tests for User Story 2 ⚠️

- [ ] T027 [P] [US2] Add fixture tests in `crates/migrate/tests/fixtures_namespace.rs` for [contracts/fixtures/namespace-copy.json](contracts/fixtures/namespace-copy.json) and invalid name-collision fixtures
- [ ] T028 [P] [US2] Add unit tests in `crates/migrate/tests/name_quota.rs` covering `NameExists` before copy, start `QuotaExceeded` before any copy, cutover `QuotaExceeded` leaving source intact and target incomplete
- [ ] T029 [P] [US2] Add unit tests in `crates/migrate/tests/move_drop.rs` that move cutover drops source (no empty shell) and copy leaves source
- [ ] T030 [P] [US2] Add conformance tests in `crates/conformance/tests/migrate_namespace.rs`: copy leaves source; move drops source; authz refuse without dual grant; name collision; start and cutover quota failures (SC-002)

### Implementation for User Story 2

- [ ] T031 [US2] Extend `MigrationSpec` in `crates/migrate/src/migrate.rs` with `dest_namespace`, `dest_name` (default = source name), `policy: copy|move`; refuse `NameExists` if dest public name taken (no overwrite/merge) per FR-002 / R10
- [ ] T032 [US2] Implement namespace copy runner in `crates/migrate/src/migrate.rs` reusing dual-write/catch-up; persist on **cluster** log when namespaces differ (R3); leave source intact on copy cutover
- [ ] T033 [US2] Implement move cutover in `crates/migrate/src/migrate.rs`: after successful cutover **drop** source via `003` (name reusable, storage released); never leave empty definition (R8)
- [ ] T034 [US2] Enforce cross-namespace authz in `crates/migrate/src/authz.rs` on create; map CLI `--to-namespace` / `--name` / `--policy` in `crates/spacestorage` and HTTP `POST /v1/migrate` bodies

**Checkpoint**: Namespace copy and move match quickstart §2; SC-002 name/quota/move behaviors green.

---

## Phase 5: User Story 3 - Rewrite a container (type, schema, codec, key, or sharding key) (Priority: P1)

**Goal**: Transform into a **new** incomplete container with five rewrite kinds; source stays writable with dual-write/catch-up; optional swap; catalog mapping for understandable pairs; mapping query → `005` `LogicalRequest` for complex; refuse in-place via `03`/`04` pointer; default drop previous after swap (`retain_source=false`).

**Independent Test**: Document→Relational Table with live writes through swap; drop field; recompress; re-encrypt; sharding-key rewrite. Complex source with mapping query; omit query → `MappingQueryRequired`. Conflicting mapping fails without modifying source.

### Tests for User Story 3 ⚠️

- [ ] T035 [P] [US3] Add fixture tests in `crates/migrate/tests/fixtures_transform.rs` for [contracts/fixtures/transform-catalog.json](contracts/fixtures/transform-catalog.json) and [contracts/fixtures/transform-mapping-query.json](contracts/fixtures/transform-mapping-query.json)
- [ ] T036 [P] [US3] Add unit tests in `crates/migrate/tests/mapping_refuse.rs`: understandable pair without query uses catalog; complex without query → `MappingQueryRequired`; missing source column/container → `MappingSourceMissing`; join/agg → `NotSupported`
- [ ] T037 [P] [US3] Add conformance tests in `crates/conformance/tests/transform_kinds.rs` covering all five rewrite kinds (type/model, incompatible schema, re-encode/recompress, re-encrypt, sharding-key) with at least one fixture each (SC-003)
- [ ] T038 [P] [US3] Add conformance tests in `crates/conformance/tests/transform_live_swap.rs`: live writes during transform appear on target at swap; swap refused until last applied source seq present
- [ ] T039 [P] [US3] Add conformance tests in `crates/conformance/tests/transform_mapping.rs`: catalog path without query; mapping-query path; missing mapping query refused; conflicting field mapping fails with source unchanged

### Implementation for User Story 3

- [ ] T040 [P] [US3] Implement `RewriteKind` and `TransformSpec` in `crates/migrate/src/transform.rs` (`type_model` | `incompatible_schema` | `re_encode` | `re_compress` | `re_encrypt` | `sharding_key`; `target_type`; `new_name`; `swap` default true same-ns; `retain_source` default **false**) per [data-model.md](data-model.md) §§6–7 and [contracts/transform.md](contracts/transform.md)
- [ ] T041 [US3] Implement catalog mapping lookup and identity maps for same-type rewrites in `crates/migrate/src/mapping.rs`; add `default_transform` on type descriptors in `crates/types` per [contracts/mapping.md](contracts/mapping.md) / R13
- [ ] T042 [US3] Implement `MappingQuery` AST (`sources[]`, `destinations[]`, optional `filter`) in `crates/migrate/src/mapping.rs` that lowers to `005` `LogicalRequest` (scan+project+insert) only — no private SQL parser; execute via `crates/exec`
- [ ] T043 [US3] Implement transform runner in `crates/migrate/src/transform.rs`: create incomplete target under free name; dual-write/catch-up with mapping; refuse colliding `new_name` (`NameExists`); composition default refuse (transform members first)
- [ ] T044 [US3] Implement swap in `crates/migrate/src/transform.rs`: atomic rename of source public name onto new container; if `retain_source=false` drop previous; if `retain_source=true` rename previous to `<old>__pre_transform_<job_id>` or refuse `NameExists` (R9)
- [ ] T045 [US3] Ensure `03`/`04` in-place incompatible schema, codec rewrite of existing data, key change, and fixed sharding-key change refuse with pointer to this transform mechanism (seams in `crates/types` / `crates/placement`)
- [ ] T046 [US3] Wire `POST /v1/transform` and CLI `spacestorage transform --from … --rewrite … [--to-type …] [--mapping-file …] [--swap|--no-swap] [--retain-source]` in admin-http + `crates/spacestorage`; `--mapping-file` is MappingQuery JSON not SQL

**Checkpoint**: Five rewrite kinds + catalog/query mapping + live swap match quickstart §§3–4; SC-003 suite green.

---

## Phase 6: User Story 4 - Backup and restore jobs invoking durability (Priority: P2)

**Goal**: `data_backup` / `data_restore` jobs schedule policy and invoke `013` snapshot/PITR APIs only — no second snapshot format; encrypted backups keep key refs; missing keys fail named.

**Independent Test**: Snapshot a namespace, drop a container, restore; content returns. PITR to WAL position (`13`). Encrypted backup without keys → fail naming reference.

### Tests for User Story 4 ⚠️

- [ ] T047 [P] [US4] Add fixture tests in `crates/migrate/tests/fixtures_backup.rs` for [contracts/fixtures/backup-namespace.json](contracts/fixtures/backup-namespace.json)
- [ ] T048 [P] [US4] Add conformance tests in `crates/conformance/tests/backup_restore_job.rs`: snapshot job completes with `snapshot_id`+WAL position from `13`; restore-in-place recovers logical data; PITR position honored (SC-005)
- [ ] T049 [P] [US4] Add conformance test in `crates/conformance/tests/backup_restore_job.rs` that encrypted backup restore without keys fails `KeyRefMissing` naming the reference

### Implementation for User Story 4

- [ ] T050 [P] [US4] Implement `BackupSpec` / `RestoreSpec` and thin orchestration in `crates/migrate/src/backup.rs`: call `013` SnapshotApi; record `snapshot_id` and WAL position on job; never write a parallel snapshot blob per [contracts/backup-restore.md](contracts/backup-restore.md) / R14
- [ ] T051 [US4] Implement restore job path in `crates/migrate/src/backup.rs` (same/new cluster as `13`; optional `pitr_position` / `key_ref`); cross-namespace restore uses same authz as cross-namespace migrate; live dest collision → `NameExists` unless `13` restore-replace accepted
- [ ] T052 [US4] Wire `POST /v1/backup`, `POST /v1/restore`, and CLI `spacestorage backup` / `spacestorage restore` in admin-http + `crates/spacestorage`; emit `08` labels `data_backup` / `data_restore` (not `snapshot`/`backup`)

**Checkpoint**: Backup/restore jobs are observable orchestration over `13`; SC-005 green.

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: Docs, wiring completeness, performance gate, quickstart validation

- [ ] T053 [P] Document evacuate/copy/transform/backup starter examples in `docs/examples/` (or link from README) aligned with [quickstart.md](quickstart.md) and contract fixtures
- [ ] T054 Ensure cancel leaves source intact and marks target incomplete (never swapped) across migrate/transform in `crates/migrate/src/lib.rs`; optional note that stale incomplete cleanup MAY be later `08` cleanup job (not required for slice 10 SC)
- [ ] T055 [P] Verify idle dual-write interceptor unregistered adds < 5% p95 on loopback KV vs `004` baseline (plan Performance Goals) via a small bench or conformance note in `crates/conformance/tests/migrate_perf_note.rs` or docs
- [ ] T056 Run full [quickstart.md](quickstart.md) validation on three-node loopback (evacuate < 20 min SC-006; copy/move; catalog + mapping-query transform; backup/restore; cancel/resume) and fix gaps in `crates/migrate` / CLI
- [ ] T057 Confirm first-binary profile still returns `MigrateSlice10Required` for all `data_*` job creates via tests in `crates/migrate/tests/first_binary_stub.rs` and `crates/release-profile` with no silent no-op

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Setup — **BLOCKS** all user stories
- **User Story 1 (Phase 3)**: Depends on Foundational — no dependency on US2–US4; **MVP**
- **User Story 2 (Phase 4)**: Depends on Foundational; reuses dual-write/migrate runner from US1 (sequential after US1 recommended)
- **User Story 3 (Phase 5)**: Depends on Foundational dual-write; mapping/transform largely parallelizable with US2 after US1 dual-write exists
- **User Story 4 (Phase 6)**: Depends on Foundational job store/authz; independent of dual-write; can proceed in parallel with US2/US3 once Phase 2 is done
- **Polish (Phase 7)**: After desired stories complete

### User Story Dependencies

- **US1 (P1)**: After Phase 2 — no other story deps — 🎯 MVP
- **US2 (P1)**: After Phase 2; shares migrate runner/dual-write with US1
- **US3 (P1)**: After Phase 2; shares dual-write; needs mapping + types catalog seams
- **US4 (P2)**: After Phase 2; only needs job lifecycle + `013` SnapshotApi

### Within Each User Story

- Tests (listed) MUST be written and FAIL before implementation
- Models/specs before runners
- Runners before HTTP/CLI wiring
- Story complete before moving to next priority when staffing is serial

### Parallel Opportunities

- Phase 1: T001–T003 mostly serial; docs/features can overlap with T002
- Phase 2: T005, T007, T008, T011, T012 parallel after T004; T009 depends on T004/T005
- After Phase 2: US4 can run parallel with US2/US3; US3 mapping unit work parallel with US2 namespace policy
- Within stories: all `[P]` test tasks parallel; snapshot/offline strategy tasks parallel with live path once MigrationSpec exists

---

## Parallel Example: User Story 1

```bash
# Launch US1 tests together:
Task: "Contract/fixture tests in crates/migrate/tests/fixtures_evacuate.rs"
Task: "Unit tests in crates/migrate/tests/dual_write_sequence.rs"
Task: "Conformance live evacuate in crates/conformance/tests/migrate_live.rs"
Task: "Anti-affinity refuse conformance in crates/conformance/tests/migrate_live.rs"
Task: "Resume conformance in crates/conformance/tests/migrate_resume.rs"

# After dual-write live path:
Task: "Snapshot strategy in crates/migrate/src/migrate.rs"
Task: "Offline strategy in crates/migrate/src/migrate.rs"
```

---

## Parallel Example: User Story 3

```bash
# Launch US3 tests together:
Task: "Fixture tests fixtures_transform.rs"
Task: "Mapping refuse unit tests mapping_refuse.rs"
Task: "Five rewrite kinds conformance transform_kinds.rs"
Task: "Live swap conformance transform_live_swap.rs"
Task: "Catalog/query mapping conformance transform_mapping.rs"

# Parallel implementation after TransformSpec:
Task: "Catalog mapping in crates/migrate/src/mapping.rs + types default_transform"
Task: "MappingQuery → LogicalRequest in crates/migrate/src/mapping.rs"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL)
3. Complete Phase 3: User Story 1 (live node evacuate + job CLI/HTTP)
4. **STOP and VALIDATE**: Independent Test for US1 + quickstart §1 (SC-006)
5. Demo evacuate without dump/reload

### Incremental Delivery

1. Setup + Foundational → foundation ready
2. US1 → live evacuate MVP
3. US2 → namespace copy/move + name/quota policy
4. US3 → five rewrite kinds + catalog/query mapping
5. US4 → backup/restore job orchestration over `13`
6. Polish → quickstart + first-binary stub confirmation

### Parallel Team Strategy

1. Team completes Setup + Foundational together
2. After Phase 2:
   - Dev A: US1 (then US2 — shared migrate runner)
   - Dev B: US3 mapping + transform (after dual-write seam from A or foundational T009)
   - Dev C: US4 backup/restore (independent of dual-write)
3. Integrate via shared `JobService` and admin routes

---

## Notes

- [P] = different files, no dependencies on incomplete tasks
- [Story] labels map to US1–US4 from [spec.md](spec.md)
- Do not invent a second snapshot format or second SQL engine
- Do not use migrate as the only replica repair path (`04`)
- First binary MUST refuse jobs with `MigrateSlice10Required`
- Commit after each task or logical group
- Stop at any checkpoint to validate the story independently

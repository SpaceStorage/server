---
description: "Task list for durability, WAL, restore, deletes, TTL, compaction, and backup"
---

# Tasks: Durability, WAL, Restore, Deletes, TTL, Compaction, and Backup

**Input**: Design documents from `/specs/013-durability-and-recovery/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md), [data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: Requested. Plan Testing section + Independent Tests + SC-001–SC-009 + [quickstart.md](quickstart.md). Unit tests in `crates/storage` / `crates/backup`; conformance in `crates/conformance`; contract fixtures under `contracts/fixtures/`. Write failing tests first where listed.

**Scope of this feature**: Own `crates/storage` (per-drive WAL, group commit, boot replay, disk-full, tombstones/`gc_grace`/TTL/compaction jobs) and new `crates/backup` (snapshot, PITR, restore-fill). Wire restore-before-`ready` from `crates/node`. Config/admin/metrics additive. Who counts toward quorum stays `012`; keys stay `014`; job orchestration of `data_backup`/`data_restore` stays `010` (slice 10). Do not invent a second LSM or rename `08` series.

**Sibling crates** (from `001`–`016` plans; create seams, do not duplicate): `crates/storage`, `crates/backup`, `crates/crypto`, `crates/placement`, `crates/replication`, `crates/clocks`, `crates/types`, `crates/config`, `crates/node`, `crates/admin-proto`, `crates/migrate`, `crates/release-profile`, `crates/conformance`.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: [US1], [US2], [US3], [US4] on user-story phase tasks only
- Every task includes an exact file path

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Crate/module skeletons, workspace membership, fixture docs, config surface stubs per [plan.md](plan.md) project structure

- [ ] T001 Create `crates/storage/src/wal/mod.rs` exporting `DriveWal` module stubs and empty `record.rs`, `group_commit.rs`, `replay.rs` under `crates/storage/src/wal/` (package `spacestorage-storage`, edition 2024)
- [ ] T002 [P] Create `crates/backup/Cargo.toml` (package `spacestorage-backup`, edition 2024) and `crates/backup/src/lib.rs` that `mod`s `manifest`, `snapshot`, `pitr`, `restore`
- [ ] T003 Add `crates/backup` to workspace `[workspace.members]` in `Cargo.toml` and depend on it from `crates/node` / `crates/admin-proto` only via public `SnapshotService` / `RestoreService` APIs (no compaction pull-through)
- [ ] T004 [P] Add empty modules `crates/storage/src/checkpoint.rs`, `crates/storage/src/tombstone.rs`, `crates/storage/src/ttl.rs`, `crates/storage/src/disk.rs`, `crates/storage/src/quarantine.rs`, and extend `crates/storage/src/restore.rs` / `crates/storage/src/compaction.rs` module declarations in `crates/storage/src/lib.rs`
- [ ] T005 [P] Copy [contracts/fixtures/single-drive.conf](contracts/fixtures/single-drive.conf) and [contracts/fixtures/two-drive.conf](contracts/fixtures/two-drive.conf) to `docs/examples/durability/{single-drive,two-drive}.conf` and document [contracts/fixtures/invalid/](contracts/fixtures/invalid/) (`sync-none.conf`) as negative startup cases

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Shared types, format headers, config directives, error codes, and metrics hooks every story needs. No user-story work until this phase is complete.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [ ] T006 Implement `DriveWal` and `WalRecord` field structs in `crates/storage/src/wal/mod.rs` and `crates/storage/src/wal/record.rs` per [data-model.md](data-model.md): `drive_id` string (`default` if only `data_dir`); `path` `{drive.path}/wal/`; `next_lsn`/`durable_lsn` `u64`; `format_major` `u16` currently `1`; `sync` `fdatasync` \| `fsync` (`none` not a production value); record `lsn`, `container_id` UUID, `stamp` HlcStamp, `type` `put` \| `delete` \| `txn_marker`, `payload` bytes
- [ ] T007 [P] Implement on-disk WAL segment header (magic `WAL1`, `format_major`, `drive_id`, creating node id) and record framing `u32le length | u32 crc32 | u16 format_major | u8 type | u64 lsn | uuid container_id | HlcStamp | u32 payload_len | payload` in `crates/storage/src/wal/record.rs` per [contracts/wal.md](contracts/wal.md) and [contracts/format.md](contracts/format.md)
- [ ] T008 [P] Implement `DurableAck { replica, drive_id, lsn, kind: durable|memory }`, `Checkpoint { drive_id, covered_lsn, at }`, and `DriveDiskState { drive_id, state: ok|full|corrupt, quarantined }` in `crates/storage/src/lib.rs` (or `ack.rs` / `disk.rs`) per [data-model.md](data-model.md)
- [ ] T009 Parse `storage { sync; gc_grace; wal_segment; group_commit { max_wait; max_bytes; }; drive … }` in `crates/config/src/` with defaults `sync=fdatasync`, `gc_grace=24h`, `wal_segment=64MiB`, `max_wait=2ms`, `max_bytes=1MiB`; refuse `gc_grace 0` as `gc_grace_too_small`; `sync none` without `SPACESTORAGE_TEST=1` → `sync_none_not_durable` per [contracts/config-directives.md](contracts/config-directives.md)
- [ ] T010 [P] Export validation/error codes in `crates/storage/src/lib.rs` (or `error.rs`): `sync_none_not_durable`, `wal_format_unsupported` / `format_unsupported`, `disk_full`, `wal_corrupt`, `pitr_unmappable`, `RestoreDestinationHasContent`, `BackupSlice10Required`, `gc_grace_too_small`
- [ ] T011 [P] Register additive metric series hooks in `crates/storage` (exposition via `008`): reserved `db_wal_*`, `db_checkpoint_*`, `db_dirty_*`, `db_unflushed_bytes`, `db_wal_lag_bytes`, `db_wal_replay_duration_seconds`, `db_wal_recovery_duration_seconds`, `db_recovery_*` plus additive `db_wal_torn_total`, `db_wal_disk_full_total` per [contracts/metrics.md](contracts/metrics.md) — MUST NOT rename `08` names
- [ ] T012 Supersede `003` per-container `ns/.../wal/` with drive-level path `{drive.path}/wal/<seq>.wal` (implicit drive `default` = `storage.data_dir`) in `crates/storage` layout helpers and document the migration note in `crates/storage/src/wal/mod.rs` per research R2

**Checkpoint**: `cargo test -p spacestorage-storage` and `cargo test -p spacestorage-backup` compile. Config validates single-/two-drive fixtures. User stories can start.

---

## Phase 3: User Story 1 - Durable acknowledgements (Priority: P1) 🎯 MVP

**Goal**: Counted persistent/hybrid write acks mean a per-drive WAL record made durable (fsync/group commit) off the Tokio worker pool; memory-mode acks are memory-only and never claimed crash-durable; one WAL per drive; mixed sets report durable vs memory.

**Independent Test**: Write persistent at `TWO`; kill process; restart; read the value. Write memory-mode; restart; confirm empty unless replicated. Fsync never occupies a database worker thread. Two-drive node: ack on drive 1 does not require a durable record on drive 2.

### Tests for User Story 1 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T013 [P] [US1] Add unit tests for group-commit coalesce (`max_wait` 2 ms / `max_bytes` 1 MiB) and `spawn_blocking` fsync fence in `crates/storage/src/wal/group_commit.rs` (or `crates/storage/tests/group_commit.rs`)
- [ ] T014 [P] [US1] Add unit tests that `sync none` yields `sync_none_not_durable` outside `SPACESTORAGE_TEST=1` and even under test never produces a counted durable ack for persistent/hybrid in `crates/storage/tests/sync_none.rs`
- [ ] T015 [P] [US1] Add conformance SC-001 crash+restart of acknowledging replicas for persistent `TWO` in `crates/conformance/tests/durability_ack.rs` using [contracts/fixtures/single-drive.conf](contracts/fixtures/single-drive.conf) / three-node `012` harness (`SPACESTORAGE_TEST=1`)
- [ ] T016 [P] [US1] Add conformance that memory-mode counted acks are `kind=memory` and execution records distinguish durable vs memory in mixed sets in `crates/conformance/tests/durability_ack.rs` per [contracts/ack.md](contracts/ack.md)

### Implementation for User Story 1

- [ ] T017 [P] [US1] Implement per-drive append path and LSN assignment in `crates/storage/src/wal/mod.rs`: records multiplex containers on that drive only; path `{drive.path}/wal/<seq>.wal`; segment rotation default 64 MiB
- [ ] T018 [US1] Implement group commit in `crates/storage/src/wal/group_commit.rs`: coalesce until `max_wait` or `max_bytes`; durability syscall (`fdatasync` default; `fsync` allowed; macOS `F_FULLFSYNC` in durability tests) exclusively via `001` `spawn_blocking` pool; advance `durable_lsn` monotonically; state machine `buffered → fsyncing → durable | failed`
- [ ] T019 [US1] Encrypt WAL payloads for encrypted containers with container data key (`014`/`003` crypto) and AAD `drive_id ‖ lsn ‖ container_id` in `crates/storage/src/wal/record.rs`; leave header/`container_id` plaintext; unencrypted containers yield plaintext payload
- [ ] T020 [US1] Wire counted persistent/hybrid ack to wait `durable_lsn ≥ record.lsn` on the replica’s pinned drive in `crates/storage` + replication/placement ack path (`crates/placement` / `crates/types`); memory-mode acks stay memory-only and MUST NOT be labelled crash-durable
- [ ] T021 [US1] Ensure hybrid counted ack covers the write (including memory portion that `WriteBuffer`/`HotSet` would lose) in `crates/storage` / `crates/types` per research R16; do not treat hybrid like memory-mode
- [ ] T022 [US1] Implement admin/CLI `spacestorage storage` and `spacestorage wal [DRIVE]` showing per-drive `durable_lsn`, lag, and disk state in `crates/admin-proto` + `crates/spacestorage` per [contracts/admin-cli.md](contracts/admin-cli.md) (slice 2)
- [ ] T023 [US1] Increment `db_wal_bytes_total`, `db_wal_fsync_total`, `db_wal_fsync_duration_seconds` on group commit in `crates/storage/src/wal/group_commit.rs` per [contracts/metrics.md](contracts/metrics.md)

**Checkpoint**: Persistent `TWO` write survives kill-9+restart of acknowledging replicas. Memory-mode is not claimed crash-durable. Fsync never runs on a Tokio worker. Drive-1 ack independent of drive-2 WAL.

---

## Phase 4: User Story 2 - Crash recovery and disk failure (Priority: P1)

**Goal**: On start restore definitions always, persistent/hybrid content from drives (WAL replay + checkpoints), memory-mode content empty unless replication re-populates; isolate corrupt files; per-drive disk-full refuses new durable writes without crash-loop or silent ack drop; unknown major refuses start; old nodes refuse newer format.

**Independent Test**: Restart with data; truncate a WAL file (torn tail); fill one drive; present unknown major version.

### Tests for User Story 2 ⚠️

- [ ] T024 [P] [US2] Add unit tests for torn-tail skip (short/CRC-invalid last record), `db_wal_torn_total` increment, and mid-log CRC → quarantine in `crates/storage/src/wal/replay.rs` / `crates/storage/tests/replay.rs`
- [ ] T025 [P] [US2] Add conformance SC-002 unreplicated memory-mode empty after restart with volatility notice in `crates/conformance/tests/recovery_boot.rs`
- [ ] T026 [P] [US2] Add conformance SC-003 disk-full on drive 1 refuses durable writes for containers on that drive while drive 2 still accepts, `node_state=degraded`, prior acks readable in `crates/conformance/tests/disk_full.rs` using [contracts/fixtures/two-drive.conf](contracts/fixtures/two-drive.conf)
- [ ] T027 [P] [US2] Add conformance SC-004 unknown major refuses start and N nodes refuse writing N+1 format in `crates/conformance/tests/format_version.rs`

### Implementation for User Story 2

- [ ] T028 [US2] Implement boot content restore sequence in `crates/storage/src/restore.rs` and wire before `ready` in `crates/node/src/lifecycle.rs`: (1) catalog restore `003`/`006`, (2) open each drive WAL, skip torn tail, replay `lsn > checkpoint` for persistent/hybrid in LSN order, (3) memory-mode empty unless `004` re-populates, (4) hybrid memory portion rebuilt per `003` HybridPolicy; parallelize per drive on blocking pool
- [ ] T029 [P] [US2] Implement torn-tail and mid-log corruption handling in `crates/storage/src/wal/replay.rs` and quarantine rename `{path}.corrupt.{unix_ts}` in `crates/storage/src/quarantine.rs` per [contracts/recovery.md](contracts/recovery.md); no crash-loop; container `degraded` or rebuild from replica
- [ ] T030 [US2] Implement ENOSPC / write-error → `DriveDiskState::full`, refuse new durable writes for containers on that drive, continue other drives and servable reads, set `node_state=degraded`, fail unfsynced group-commit waiters without counted ack in `crates/storage/src/disk.rs`; increment `db_wal_disk_full_total`
- [ ] T031 [P] [US2] Enforce `format_major` on open: unknown major → refuse node start naming the path (`format_unsupported`); refuse writing a major belonging to product N+1 on node N in `crates/storage/src/wal/mod.rs` per [contracts/format.md](contracts/format.md) / `015`
- [ ] T032 [US2] Implement checkpoint persist of `covered_lsn` and conditional WAL prefix truncate in `crates/storage/src/checkpoint.rs` only when R5 retain rules pass (`gc_grace`, replica/follower lag, snapshot pins)
- [ ] T033 [US2] Increment `db_wal_replay_duration_seconds`, `db_wal_recovery_duration_seconds`, `db_recovery_total`, `db_recovery_duration_seconds`, `db_recovery_records_total` during boot in `crates/storage/src/restore.rs` / `crates/node`

**Checkpoint**: Restart restores durable content; memory-mode empty; torn tail skipped; one full drive degrades without killing the node or other drives; unknown major refuses start.

---

## Phase 5: User Story 3 - Deletes, TTL, compaction (Priority: P2)

**Goal**: Deletes visible once enough replicas applied the tombstone; compaction drops tombstones only after source-domain visibility **and** `gc_grace` (default 24h) **and** follower catch-up rules; TTL expiry becomes tombstones via `TTL expiration` job; null / missing / tombstone stay distinct.

**Independent Test**: Delete; compact too early (must retain); wait grace; compact; TTL expire via background job.

### Tests for User Story 3 ⚠️

- [ ] T034 [P] [US3] Add unit tests that premature compaction retains tombstones still required by source-domain replicas or uncaught-up followers in `crates/storage/tests/gc_grace.rs`
- [ ] T035 [P] [US3] Add conformance SC-005 premature compaction retain + post-grace drop (test override e.g. `gc_grace` 1s) in `crates/conformance/tests/tombstones.rs`
- [ ] T036 [P] [US3] Add unit/conformance that TTL expiry enqueues a delete/tombstone via job `TTL expiration` (never silent omit) and uses event-time if present else ingest/HLC in source domain in `crates/storage/tests/ttl.rs`

### Implementation for User Story 3

- [ ] T037 [P] [US3] Implement `Tombstone` with `gc_grace_until = created + container/namespace/storage.gc_grace` (default **24h**) in `crates/storage/src/tombstone.rs` per [data-model.md](data-model.md) and [contracts/tombstones.md](contracts/tombstones.md)
- [ ] T038 [US3] Gate tombstone drop in `crates/storage/src/compaction.rs`: MAY drop only if (1) every source `quorum_domain` replica has seen it or was rebuilt from a newer snapshot, (2) `now ≥ gc_grace_until`, (3) async followers caught up on source log or rebuilt-from-snapshot; consult `crates/placement` / `crates/replication` lag
- [ ] T039 [US3] Implement `TTL expiration` job in `crates/storage/src/ttl.rs` emitting tombstones; clock = type event-time if present else ingest/HLC in source domain; MUST NOT use TTL event-time as PITR clock
- [ ] T040 [P] [US3] Wire background jobs `flush`, `checkpoint`, `compaction`, `vacuum`, `GC`, `TTL expiration` on the blocking pool in `crates/storage` with `spacestorage_job_*` labels per [contracts/jobs.md](contracts/jobs.md) and [contracts/compaction.md](contracts/compaction.md); strategy defaults remain type catalog `003`
- [ ] T041 [US3] Preserve null / missing field / tombstone as three distinct `003` value states through compaction paths in `crates/storage/src/compaction.rs` / `crates/types` (FR-012)

**Checkpoint**: Early compaction retains required tombstones; after grace + visibility, drop allowed; TTL creates tombstones via the named job.

---

## Phase 6: User Story 4 - Snapshot and PITR (Priority: P2)

**Goal**: Snapshot records independent per-drive WAL positions without pausing other drives; includes persistent/hybrid content + all definitions/options; omits memory-mode content; PITR by per-drive LSN or source-domain HLC/ingest mapped per drive; restore refuses overwrite without `confirm_drop`; encrypted backups use key refs; library+tests in slice 2, operator jobs slice 10.

**Independent Test**: Snapshot; drop; restore. PITR to WAL position and HLC timestamp. Encrypted snapshot without key fails naming the ref. Snapshot memory-mode → restore empty content.

### Tests for User Story 4 ⚠️

- [ ] T042 [P] [US4] Add unit tests for HLC→LSN map (`lsn* = max { lsn | stamp ≤ T }`), refuse `pitr_unmappable` naming drive, and never past last durable ack in `crates/backup/src/pitr.rs` / `crates/backup/tests/pitr.rs`
- [ ] T043 [P] [US4] Add unit tests for `confirm_drop=false` → `RestoreDestinationHasContent` (dest intact) and confirm-drop/empty → fill in `crates/backup/tests/restore.rs`
- [ ] T044 [P] [US4] Add conformance SC-006 multi-drive independent positions (no cluster freeze), PITR HLC map, and unmappable-drive refuse in `crates/conformance/tests/snapshot_pitr.rs`
- [ ] T045 [P] [US4] Add conformance SC-007 missing key fails naming the reference, SC-008 overwrite refuse, SC-009 memory-mode content omitted from snapshot/restore empty in `crates/conformance/tests/snapshot_pitr.rs`

### Implementation for User Story 4

- [ ] T046 [P] [US4] Implement `SnapshotManifest` (`snapshot_id`, `scope` container\|namespace, `positions { drive_id: lsn }`, `containers`, `key_refs`, `format_major`) in `crates/backup/src/manifest.rs` per [data-model.md](data-model.md); magic `SNP1` major 1
- [ ] T047 [US4] Implement `SnapshotService` in `crates/backup/src/snapshot.rs`: record per-drive LSN **without** pausing other drives; hardlink/copy persistent/hybrid SSTables; include definitions/options of every in-scope container; **omit memory-mode content**; pin WAL/SSTables for PITR; artifact under `{data_dir}/snapshots/<snapshot_id>/`
- [ ] T048 [US4] Implement `PitrTarget` (`positions` \| `hlc`) and mapping in `crates/backup/src/pitr.rs` per [contracts/pitr.md](contracts/pitr.md); fail whole job if any involved drive cannot map; never apply past last durable ack; TTL event-time / wall clock are not PITR clocks
- [ ] T049 [US4] Implement `RestoreService` in `crates/backup/src/restore.rs`: same-cluster and new-cluster fill; refuse while dest has content unless `confirm_drop=true`; after drop/empty fill names; accept specified key ref; missing key fails naming reference; cross-namespace requires admin (`014`)
- [ ] T050 [US4] Encrypt snapshot files with same key references as source containers (`014`) in `crates/backup/src/snapshot.rs`; document unrestorable encrypted data when keys lost
- [ ] T051 [US4] Expose library APIs for `010` to store the **position map** (not a single LSN) from `crates/backup/src/lib.rs`; in first-binary profile, admin `snapshot`/`restore`/`snapshots` return `BackupSlice10Required` in `crates/admin-proto` / `crates/spacestorage` per [contracts/admin-cli.md](contracts/admin-cli.md); slice-10 wiring calls these APIs from `crates/migrate` without a second snapshot format
- [ ] T052 [P] [US4] Add HTTP `/v1/storage`, `/v1/wal` (slice 2) and stub `/v1/snapshots` (slice 10) in `crates/admin-proto` / `crates/node` matching CLI verbs

**Checkpoint**: Snapshot manifests carry independent per-drive positions; memory content absent; PITR HLC map fail-closed; restore overwrite refused without confirm-drop; missing key named. Operator backup jobs may wait for slice 10.

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: Docs, profile flags, quickstart validation, release-profile slice markers

- [ ] T053 [P] Document durability/restore operator procedures and quickstart walkthrough in `docs/` linking [quickstart.md](quickstart.md) (SC-001–SC-009 commands)
- [ ] T054 Mark slice 2 (WAL+restore) required and slice 10 (operator snapshot jobs) deferred in `crates/release-profile` per research R14 / `016`
- [ ] T055 [P] Ensure `storage.sync none` is rejected at startup for persistent/hybrid outside tests across `crates/config` + `crates/node` using [contracts/fixtures/invalid/sync-none.conf](contracts/fixtures/invalid/)
- [ ] T056 Run [quickstart.md](quickstart.md) validation end-to-end via `crates/conformance/tests/{durability_ack,recovery_boot,disk_full,format_version,tombstones,snapshot_pitr}.rs` (`cargo test -p spacestorage-conformance`) and fix gaps
- [ ] T057 [P] Confirm no `08` series renamed and job labels match [contracts/jobs.md](contracts/jobs.md) / [contracts/metrics.md](contracts/metrics.md) in metrics registration code

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately
- **Foundational (Phase 2)**: Depends on Setup completion — BLOCKS all user stories
- **User Stories (Phase 3–6)**: All depend on Foundational phase completion
  - US1 (P1) and US2 (P1) are both first-binary critical; prefer US1 → US2 sequentially (replay needs WAL)
  - US3 (P2) can start after Foundational; benefits from US1 WAL records existing
  - US4 (P2) depends on US1 WAL LSNs + US2 checkpoints for meaningful snapshots; library can stub earlier
- **Polish (Phase 7)**: Depends on desired user stories being complete

### User Story Dependencies

- **User Story 1 (P1)**: After Foundational — no dependency on other stories — 🎯 MVP
- **User Story 2 (P1)**: After Foundational; practically after US1 WAL append/group-commit exist for replay to exercise
- **User Story 3 (P2)**: After Foundational; needs delete WAL records (US1) and optionally lag signals from `012`/`004`
- **User Story 4 (P2)**: After Foundational; needs durable LSNs (US1) and preferably checkpoints (US2); operator CLI jobs wait slice 10 / `010`

### Within Each User Story

- Tests (where listed) MUST be written and FAIL before implementation
- Models/types before services
- Services before admin/CLI endpoints
- Core implementation before cross-crate integration
- Story complete before moving to next priority when staffing is serial

### Parallel Opportunities

- Phase 1: T002, T004, T005 in parallel after T001/T003 scaffolding starts
- Phase 2: T007, T008, T010, T011 in parallel once T006 types exist
- US1 tests T013–T016 in parallel; US2 tests T024–T027 in parallel; US3 tests T034–T036 in parallel; US4 tests T042–T045 in parallel
- After Foundational: US3 tombstone types (T037) can proceed while US1 group commit lands if interfaces are stable
- US4 manifest (T046) parallel with US4 PITR unit tests (T042)

---

## Parallel Example: User Story 1

```bash
# Launch all US1 tests together:
Task: "Unit tests group-commit in crates/storage/src/wal/group_commit.rs"
Task: "Unit tests sync none in crates/storage/tests/sync_none.rs"
Task: "Conformance SC-001 in crates/conformance/tests/durability_ack.rs"
Task: "Conformance memory vs durable ack kinds in crates/conformance/tests/durability_ack.rs"

# Then implementation (group commit before ack wiring):
Task: "Per-drive append in crates/storage/src/wal/mod.rs"
Task: "Group commit + spawn_blocking fsync in crates/storage/src/wal/group_commit.rs"
```

---

## Parallel Example: User Story 4

```bash
# Launch US4 tests together:
Task: "PITR HLC map unit tests in crates/backup/tests/pitr.rs"
Task: "confirm_drop restore unit tests in crates/backup/tests/restore.rs"
Task: "Conformance SC-006–SC-009 in crates/conformance/tests/snapshot_pitr.rs"

# Launch independent models/APIs:
Task: "SnapshotManifest in crates/backup/src/manifest.rs"
Task: "Admin HTTP stubs in crates/admin-proto"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL — blocks all stories)
3. Complete Phase 3: User Story 1 (durable per-drive WAL + group commit + counted acks)
4. **STOP and VALIDATE**: crash+restart persistent `TWO`; memory-mode not crash-durable
5. Demo/ship slice-2 core durability contract

### Incremental Delivery

1. Setup + Foundational → types/config/format ready
2. US1 → durable acks (MVP)
3. US2 → boot replay, disk-full, format refuse (constitution XII content half)
4. US3 → tombstones / `gc_grace` / TTL jobs
5. US4 → snapshot/PITR library (+ slice-10 operator jobs via `010`)
6. Each story adds value without breaking prior durable-ack guarantees

### Parallel Team Strategy

With multiple developers:

1. Team completes Setup + Foundational together
2. Once Foundational is done:
   - Developer A: User Story 1 (WAL / group commit)
   - Developer B: User Story 2 scaffolding (quarantine/disk) behind WAL trait
   - Developer C: User Story 3 tombstone/`gc_grace` types
   - Developer D: User Story 4 `crates/backup` manifest/PITR stubs
3. Integrate US2 replay onto real `DriveWal` after US1 lands

---

## Notes

- [P] tasks = different files, no dependencies on incomplete tasks
- [Story] label maps task to specific user story for traceability
- First binary (`016` slices 1–5 / slice 2): US1 + US2 (+ local US3) required; US4 library+tests in-tree, admin backup jobs slice 10
- `010` MUST call this snapshot/PITR API (position **map**); do not invent a second snapshot format
- Avoid: fsync on Tokio workers, node-global disk-full, cluster-wide snapshot freeze, memory-mode content in snapshots, silent overwrite restore, renaming `08` metrics

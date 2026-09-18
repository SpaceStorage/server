# Implementation Plan: Durability, WAL, Restore, Deletes, TTL, Compaction, and Backup

**Branch**: `013-durability-and-recovery` | **Date**: 2026-09-18 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/013-durability-and-recovery/spec.md` (Clarifications, Sessions 2026-09-15 and 2026-09-18 — durable ack = WAL fsync/group commit; **one WAL per drive**; `gc_grace`; independent per-drive snapshots; restore refuse-while-content; no memory-mode snapshot content; PITR = source-domain HLC mapped per drive)

## Summary

Own the **durability contract** that constitution II/XII and `003` restore sketched: a counted write ack for persistent/hybrid data is a **per-drive WAL record that has been made durable** (fsync or equivalent group commit) off the Tokio worker pool; memory-mode acks stay memory-only; crash restart restores definitions always, durable content from media + WAL replay, memory content empty; tombstones drop only after source-domain visibility **and** `gc_grace`; snapshots are crash-consistent **per drive** (no cluster freeze, no memory-mode bytes); same-cluster restore refuses overwrite unless confirm-drop; PITR maps an HLC/ingest timestamp to a WAL LSN per drive.

`003` still owns layouts (memtable/SSTable/LSM), hybrid **policy**, and the catalog log. This feature **replaces** `003`'s per-container `wal/` directory with one multiplexed WAL **per drive** (`004` drive blocks). `004`/`012` still decide **which replicas count**; this crate decides **what an ack means on disk**. `010` schedules `data_backup`/`data_restore` jobs and **must call** this snapshot/PITR API (position **map**, not a single LSN). `014` supplies data keys for WAL/snapshot records. `008` owns series **names**; this feature increments `db_wal_*`, `db_checkpoint_*`, `db_dirty_*`, `db_unflushed_*`, `db_recovery_*` and the compaction/TTL/snapshot/backup/restore **jobs**.

**First binary** (`016` slices 1–5 / slice 2): per-drive WAL, group-commit fsync, boot replay, disk-full and corruption behavior, format version, local `gc_grace` (single node = the whole source domain). **Slice 10**: operator snapshot/PITR jobs via `010`; mechanics and in-crate tests exist here so `010` has an API.

## Technical Context

**Language/Version**: Rust 1.87 (edition 2024, MSRV 1.85), same Cargo workspace as `001`–`016`.

**Primary Dependencies**: existing workspace (`tokio`, `async-trait`, `serde`/`serde_json`, `bytes`, `tracing`, `uuid`, `parking_lot`, `crc32fast`). WAL record encryption via `003` `crates/crypto` + `014` `KeyAuthority` (first binary: master-key file). No `*-sys`, no Direct I/O, no second LSM. Fsync/fdatasync via `std::fs::File` on `spawn_blocking` (existing `001` blocking pool). Checksums: CRC32 on WAL headers (same as `003` catalog).

**Storage**: One WAL stream **per drive** that hosts persistent or hybrid data: `{drive.path}/wal/<seq>.wal`. SSTables/segments stay under `{drive.path}/ns/<namespace>/<container-id>/` (`003` layout, path root moved from a single `data_dir` to the **drive** that placement pinned). Catalog log remains `003`/`006` (definitions). Snapshot artifacts: `{data_dir}/snapshots/<snapshot_id>/` with `manifest.json` (per-drive LSNs) + hardlink/copy of in-scope SSTables; WAL segments after those LSNs retained for PITR. Memory-mode content is never in a snapshot.

**Testing**: `cargo test`. Unit: group commit, torn-tail truncate, per-drive isolation, gc_grace retain, HLC→LSN map, confirm-drop. `crates/conformance`: crash+restart of `TWO` acks (SC-001); memory-mode empty (SC-002); disk-full on drive 1 vs write on drive 2 (SC-003); unknown major refuse (SC-004); premature compaction (SC-005); snapshot/PITR/HLC map (SC-006); missing key (SC-007); restore overwrite refuse (SC-008); memory-mode omitted from snapshot (SC-009). Contract tests on `contracts/fixtures/`.

**Target Platform**: Linux x86_64/aarch64 servers (primary), macOS for development. Multi-drive tests: two directories as drives on one in-process node (loopback), same harness as `004`/`016`. Kill-9 + restart for crash tests (`SPACESTORAGE_TEST=1`).

**Project Type**: Cargo workspace extension — **own** `crates/storage` (WAL, checkpoint, compaction/TTL/`gc_grace`, boot content restore) and **new** `crates/backup` (`spacestorage-backup`: snapshot, PITR, restore). No new binary. Wired from `node` restore-before-`ready`. Additive admin/CLI/config.

**Performance Goals**: group-commit wait default **2 ms** or **1 MiB** (whichever first); counted persistent ack includes that fsync, not a memtable insert. Fsync **never** on a Tokio worker. Single-node crash+restart of a small KV fixture (≤ 10 k keys) ready < 5 s. Snapshot of that fixture < 10 s. PITR apply bounded by retained WAL bytes (`db_wal_lag_bytes`).

**Constraints**: Counted persistent/hybrid ack MUST wait durable WAL on that replica's **drive**. `storage.sync none` MUST NOT produce counted durable acks (test-only or refuse). WAL multiplexes containers on a drive; each record uses that container's data key when encrypted. Unknown on-disk **major** → refuse start. Old node MUST NOT write a newer format (`015` N/N+1). Snapshot MUST NOT pause other drives. Restore MUST NOT overwrite content without confirm-drop. PITR timestamp is source-domain HLC/ingest, not wall clock, not TTL event-time. First binary MUST include WAL+restore (slice 2); operator backup jobs MAY wait for slice 10.

**Scale/Scope**: 1 WAL per drive; group commit; checkpoints; tombstone/`gc_grace`/TTL jobs; snapshot+PITR; restore procedures. Roughly: per-drive WAL + group commit (~4 k), replay/corruption/disk-full (~2.5 k), compaction/`gc_grace`/TTL (~3 k), backup crate (~4 k), config/admin/metrics (~2 k), conformance (~3.5 k); ≈ 18–22 k lines including tests.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Gate | Status |
|---|-----------|------|--------|
| I | Rust-Only Stack | WAL/snapshot in-house; CRC32/`crypto` already pure Rust; no `librocksdb`/`-sys` | PASS |
| II | Fully Asynchronous Tokio Runtime | Request path `async`; **fsync/fdatasync on `spawn_blocking`**; WAL append may use `tokio::fs` until the durability syscall | PASS |
| III | Single-Process Multithreaded Monolith | Libraries in `spacestoraged`; compaction/TTL/snapshot are jobs, not sidecars | PASS |
| IV | Type-Driven Multiparadigm | No new datatype hierarchy. Compaction **strategy** stays type-catalog (`003`). Tombstone/null/missing remain `003` value domains | PASS |
| V | Protocol Compatibility on Distinct Ports | No new client protocol. Snapshot/restore on admin CLI/HTTP | PASS |
| VI | Every Node Is a Request Coordinator | Every member restores its local drives; coordinators wait durable acks (`012` who counts) | PASS |
| VII | Label-Based Planetary Placement | WAL lives on the **drive** placement pinned (`004` media labels). Disk-full is per drive, not node-global | PASS |
| VIII | Cassandra-Style Quorum with Protocol Defaults | `TWO`/`QUORUM` still count only **durable** source-domain replicas (`012`). This feature defines durable | PASS |
| IX | Multi-Tenant Namespaces | WAL records carry namespace/container; snapshot scope is container or namespace (`010`); cross-namespace restore needs admin (`014`) | PASS |
| X | Observability as a Product Surface | Increment `08` durability + job series; no rename | PASS |
| XI | Documented, Expandable Configuration | `storage.sync`, `gc_grace`, group-commit, WAL segment; starters with one and two drives | PASS |
| XII | Raft Controller Elections and Local Restore | **This feature is the content half of XII**: WAL replay + checkpoints. Definitions stay `003`/`006`. Memory-mode empty. Data path still leaderless | PASS |
| XIII | Security Defaults for Data and Roles | WAL/snapshot payloads use container data keys (`014`). Unencrypted WAL is the operator-chosen leak | PASS |
| Arch. Contracts | Five-level stack | WAL/SSTable remain L0 primitives inside layouts; backup is an operational capability, not a type | PASS |
| Observability Contract | No `08` series renamed | Additive use of reserved `db_wal_*` / job names only | PASS |

**Gate result (pre-research)**: PASS. Sequenced deviations (per-drive WAL vs `003` per-container `wal/`; `sync none` vs `003` three-way sync; operator snapshot jobs in slice 10) are in Complexity Tracking.

## Project Structure

### Documentation (this feature)

```text
specs/013-durability-and-recovery/
├── plan.md
├── research.md                     # Phase 0: R1–R16
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── wal.md                      # per-drive stream, record, group commit, fsync
│   ├── ack.md                      # durable vs memory counted acks
│   ├── recovery.md                 # boot, torn tail, corruption, disk-full
│   ├── format.md                   # on-disk version, N/N+1
│   ├── tombstones.md               # gc_grace, TTL clock vs PITR clock
│   ├── compaction.md               # flush/checkpoint/vacuum/GC jobs
│   ├── snapshot.md                 # per-drive LSN map, no memory content
│   ├── pitr.md                     # HLC/ingest → LSN
│   ├── restore.md                  # confirm-drop, same/new cluster
│   ├── config-directives.md
│   ├── admin-cli.md
│   ├── metrics.md
│   ├── jobs.md                     # 08 job names this feature runs
│   └── fixtures/
│       ├── README.md
│       ├── single-drive.conf
│       ├── two-drive.conf
│       └── invalid/
├── checklists/requirements.md
└── tasks.md                        # Phase 2 (/speckit-tasks — NOT created here)
```

### Source Code (repository root)

```text
crates/
├── storage/                                 # spacestorage-storage — OWNED here (started in 003)
│   └── src/
│       ├── wal/
│       │   ├── mod.rs                       # DriveWal: one stream per drive
│       │   ├── record.rs                    # header + payload; encrypt via crypto
│       │   ├── group_commit.rs              # wait max_wait / max_bytes; spawn_blocking fsync
│       │   └── replay.rs                    # torn-tail skip; LSN order
│       ├── checkpoint.rs                    # covered prefix truncate (gc_grace + repair + PITR pins)
│       ├── tombstone.rs                     # gc_grace; source-domain / follower lag
│       ├── ttl.rs                           # TTL expiration job
│       ├── compaction.rs                    # existing LSM compaction; tombstone drop gate
│       ├── restore.rs                       # content restore (003 catalog first)
│       ├── disk.rs                          # ENOSPC → per-drive refuse; node_state degraded
│       └── quarantine.rs                    # corrupt file isolate
│
├── backup/                                  # spacestorage-backup — NEW
│   └── src/
│       ├── lib.rs                           # SnapshotService, RestoreService
│       ├── manifest.rs                      # snapshot_id, positions{drive→lsn}, containers
│       ├── snapshot.rs                      # hardlink/copy sst; no cluster freeze
│       ├── pitr.rs                          # HLC map; refuse unmappable drive
│       └── restore.rs                       # confirm_drop; fill names
│
├── crypto/                                  # WAL record encrypt/decrypt (003); key refs 014
├── placement/                               # drive pin; replica catch-up lag for gc_grace
├── replication/                             # source-log apply lag so source does not drop tombstones
├── clocks/                                  # HLC stamp on WAL records (PITR map)
├── types/                                   # hybrid policy unchanged; durable ack wait storage
├── config/                                  # storage.sync, gc_grace, group_commit, wal_segment
├── node/                                    # restore WAL before ready; fsync pool
├── admin-proto/                             # Snapshot, Restore, WalStatus
├── migrate/                                 # 010 jobs call backup APIs (slice 10)
├── release-profile/                         # slice 2: WAL+restore; slice 10: snapshot CLI jobs
└── conformance/                             # SC-001–SC-009
```

**Structure Decision**: Keep LSM/SSTable code in `storage` (one engine). WAL becomes a **drive-level** object those layouts append to, not a per-container file tree. Snapshot/PITR is a separate crate so `010` and admin depend on backup without pulling compaction. `003` catalog restore still runs first; this crate then replays each drive WAL.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| Per-drive multiplexed WAL instead of `003` `ns/.../wal/` | Clarify 2026-09-18 Q1; disk-full and fsync isolation | Keeping per-container WALs would fsync N files per ack and ignore drive failure domains |
| `storage.sync none` does not count as durable (override `003` three-way sync) | Spec FR-001: counted ack = fsync/group commit | Allowing `none` would let `TWO` lie after kill-9 |
| Operator snapshot/PITR jobs sequenced to slice 10 (`016`) | First binary is WAL+restore on one/three nodes | Shipping backup jobs in slice 2 would pull `010` job runner into the first binary |
| Independent per-drive snapshot (no freeze) | Clarify Q2 | Coordinated fence is a later consistency product; `05` transactions are a separate cut |
| Catalog WAL stays `003`/`006`, data WAL is this feature | Definitions vs content (constitution XII split) | One log for catalog+data would couple schema ack to data fsync and break `006` Raft |

## Constitution Check (post-design)

Re-evaluated after Phase 1: fsync only on the blocking pool; one WAL per drive; counted persistent acks wait that WAL; memory-mode never claimed crash-durable or snapshotted as content; restore definitions (`003`) then WAL replay; tombstones gated by `gc_grace`; snapshot position map; PITR uses domain HLC; metrics additive. **Gate result: PASS.**

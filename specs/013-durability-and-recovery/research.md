# Research: Durability, WAL, Restore, Deletes, TTL, Compaction, and Backup

**Feature**: `013-durability-and-recovery` | **Date**: 2026-09-18

Each item resolves a Technical Context unknown or a technology choice. Format: Decision / Rationale / Alternatives considered. Inputs: [spec.md](spec.md) (clarify 2026-09-15 and 2026-09-18), constitution 1.3.0, intent `13`, sibling plans `003` (storage layout, per-container WAL sketch, hybrid policy, catalog restore), `004` (drives, durable vs memory ack filter), `008` (series names), `010` (backup jobs call this API), `012` (who counts, source log, HLC), `014` (data keys), `015` (format N/N+1), `016` (slice 2 vs slice 10).

No `NEEDS CLARIFICATION` remains in Technical Context.

## R1. Own `storage`, add `backup`

- **Decision**: **Own** `crates/storage` (introduced in `003`). Add `crates/backup`. Compaction/TTL/`gc_grace`/WAL/replay stay in `storage`. Snapshot/PITR/restore-fill live in `backup` and are the only snapshot format `010` may call.
- **Rationale**: Spec splits durability syscalls from backup-as-product. `010` already forbids a second snapshot format.
- **Alternatives considered**: New `wal` crate only (rejected: LSM flush/checkpoint share LSNs); snapshot inside `storage` (rejected: `010`/`admin` would depend on compaction).

## R2. Per-drive WAL replaces `003` per-container `wal/`

- **Decision**: One append-only stream on each `004` **drive** that hosts persistent or hybrid data: `{drive.path}/wal/<seq>.wal`. Records multiplex containers (header includes `container_id`). SSTables stay `{drive.path}/ns/<namespace>/<container-id>/sst/`. If config has only `storage.data_dir` and no `drive` blocks, that directory is implicit drive **`default`** (one WAL). A write is appended to the WAL of the drive **this replica's durable copy** lives on (`004` pin).
- **Rationale**: Clarify Q1; disk-full and group commit are per failure domain. `003` `ns/.../wal/` is **superseded**.
- **Alternatives considered**: One WAL per node (rejected: one full data disk would not isolate fsync); one WAL per container (rejected: N fsyncs per coordinator ack); one WAL per namespace (rejected: mixed media on one stream).

## R3. Group commit and `spawn_blocking` fsync

- **Decision**: Default `storage.sync = fdatasync` (POSIX `fdatasync`; on macOS `F_FULLFSYNC` when available in tests that assert durability). `fsync` allowed. `none` **MUST NOT** produce a counted durable ack; outside `SPACESTORAGE_TEST=1` a persistent/hybrid container with `none` is a **startup error** (`sync_none_not_durable`). Group commit: coalesce waiting acks until **`max_wait` 2 ms** or **`max_bytes` 1 MiB**, then one durability syscall on the **`001` blocking pool**. Tokio workers only enqueue; they never call `fsync`. WAL payload write may use `tokio::fs` or a dedicated writer thread; the ack fence is the blocking syscall.
- **Rationale**: Constitution II; spec FR-001/FR-005. `003` allowed `sync none` as a third mode — that would lie to `TWO`.
- **Alternatives considered**: fsync every record (latency); `io_uring` (portability, extra deps); keep `none` for production (rejected).

## R4. WAL record and encryption

- **Decision**: Record: `u32le length | u32 crc32 | u16 format_major | u8 type | u64 lsn | uuid container_id | HlcStamp | u32 payload_len | payload`. Types: `put`, `delete` (tombstone), `txn_marker` (reserved for `005`). CRC over header+payload after encryption. Encrypted container: payload is AEAD with that container's **data key** (`014`/`003` crypto); AAD = `drive_id ‖ lsn ‖ container_id`. Unencrypted container: plaintext payload (operator-chosen leak). Header fields including `container_id` stay plaintext so replay can skip unknown/unreadable containers.
- **Rationale**: FR-004; multiplexed log must route without unwrapping every key.
- **Alternatives considered**: Encrypt whole record (rejected: cannot skip other tenants on replay); one key per drive (rejected: spec per-container data key).

## R5. LSN, rotation, retention, torn tail

- **Decision**: Per-drive **LSN** `u64` monotonic, assigned before enqueue. Segment size default **64 MiB**. Rotation on size or admin checkpoint. Prefix truncate only when **all** of: (1) checkpoint/SSTable image covers that LSN for every container on the drive, (2) `gc_grace` elapsed for tombstones in that prefix, (3) no replica catch-up / hinted-handoff / follower source-log consumer still needs it (`004`/`012` lag), (4) no snapshot **pin** for PITR. **Torn tail**: last record with short read or CRC failure is **skipped**, LSN does not advance past it, `db_wal_torn_total` increments, start continues (`003` catalog already does this).
- **Rationale**: Spec FR-003; deferred torn-write default. Cassandra/Postgres both truncate a torn tail rather than refuse start.
- **Alternatives considered**: Refuse start on torn tail (ops nightmare); retain WAL forever (disk-full).

## R6. On-disk format version

- **Decision**: Magic `WAL1` / `SST1` already in `003` blocks. WAL segment header carries **`format_major`**. Current major **1**. Unknown major → **refuse start** (`format_unsupported`). A node at product N MUST NOT write major belonging to N+1 (`015`). N/N+1 mixed cluster: new nodes read major 1; they MAY write major 1 until all members understand 2.
- **Rationale**: FR-008. Matches `012` internodes N/N+1 window.
- **Alternatives considered**: Silent rewrite on open (data loss); sidecar converter process (Principle III).

## R7. Disk-full and corruption

- **Decision**: `ENOSPC` / write error on a drive → refuse **new durable writes** for containers whose copy on this node is on that drive (`disk_full`); other drives continue; servable reads continue; `node_state=degraded` (`disk_full`). In-flight group commit that **already** fsynced counts; unfsynced waiters fail `disk_full` and MUST NOT be counted. Corrupt SSTable/WAL segment: rename `*.corrupt.<ts>`, do not use as truth; rebuild from replica (`004`/`012`) or mark container `degraded`. No crash-loop. No silent drop of already-counted acks.
- **Rationale**: FR-007; per-drive WAL.
- **Alternatives considered**: Node-global disk-full (rejected: clarify Q1); kill process on ENOSPC.

## R8. Boot restore sequence

- **Decision**: `node` before `ready`: (1) `003` catalog restore (definitions/options always), (2) open each drive WAL, skip torn tail, replay records with LSN > checkpoint for persistent/hybrid containers on that drive, (3) memory-mode content **empty** unless `004` re-populates, (4) hybrid memory portion rebuilt per `003` `HybridPolicy` from durable data + WAL (memtable from WAL). Parallel replay **per drive** on the blocking pool; containers on one WAL replay in LSN order.
- **Rationale**: Constitution XII; spec FR-006. `003` R7/R8 remain for catalog and hybrid policy.
- **Alternatives considered**: Replay per container in parallel on one WAL (ordering bugs).

## R9. `gc_grace` default 24 h

- **Decision**: Default **`gc_grace = 24h`**, namespace/container override. Compaction MAY drop a tombstone only if (1) every replica in the **source `quorum_domain`** has seen it or was rebuilt from a snapshot newer than the delete, **and** (2) grace elapsed, **and** (3) every async follower that still consumes the source log has applied past that delete **or** is declared rebuilt-from-snapshot (`012`). Single-node first binary: (1) is the local replica.
- **Rationale**: Spec “hours-scale”. Cassandra’s 10 d is a repair-window default; here repair retention is `004` and WAL retain is R5 — 24 h is the tombstone floor operators can raise.
- **Alternatives considered**: 10 days (not hours-scale); 0 (resurrects data).

## R10. TTL clock vs PITR clock

- **Decision**: **TTL**: event-time field if the type has one; else ingest/HLC **in the source domain**. Expiry enqueues a **delete/tombstone** via job `TTL expiration` (`08`), never a silent omit. **PITR**: source-domain **HLC or ingest stamp on the WAL record**, never TTL event-time, never operator wall clock.
- **Rationale**: Spec FR-010 vs clarify PITR Q5.
- **Alternatives considered**: One clock for both (wrong for event-time series).

## R11. Snapshot: per-drive LSN map, no freeze, no memory bytes

- **Decision**: Snapshot records `{ drive_id → lsn }` **without** pausing other drives. Crash-consistency is **per drive**. Manifest lists in-scope container ids, definitions/options, key references, format major. Persistent/hybrid **content** = SSTables/segments as of that drive’s LSN (hardlink if same filesystem, else copy) plus WAL from that LSN if PITR requested later. **Memory-mode content omitted**; definitions included. Scope: container or namespace (`010`). Incremental MAY be later; **full snapshot MUST** exist. `010` job stores the **position map**, not a single LSN (this contract supersedes the singular wording in `010` backup-restore.md).
- **Rationale**: Clarify Q2 and Q4. Independent cuts can mix writes across drives; that is explicit in the spec.
- **Alternatives considered**: Coordinated fence (rejected in specify); include memory dump (rejected).

## R12. Restore confirm-drop

- **Decision**: Restore into same or new cluster. If a destination **name still has content** → `RestoreDestinationHasContent` unless `confirm_drop=true` (drops then fills). Empty name or missing name → fill. Live writes to those names are gone before fill (drop removes the container). Cross-namespace: admin (`014`). Encrypted snapshot: restore with specified key ref; missing key fails naming the reference.
- **Rationale**: Clarify Q3; `010` restore-after-drop.
- **Alternatives considered**: Online overwrite (mixes live writes with snapshot bytes); always-new names (`010` swap only).

## R13. PITR HLC map

- **Decision**: Target is either explicit `{ drive → lsn }` **or** one source-domain HLC/ingest timestamp `T`. For each involved drive, LSN* = max { lsn | record.stamp ≤ T }. If a drive has no mappable stamps (empty WAL after snapshot, missing clock field) → **refuse whole job** naming the drive; no drive is applied past the snapshot under that failed job. Never past last durable ack LSN on that drive.
- **Rationale**: Clarify Q5. Fail-closed so a namespace PITR cannot silently skip a drive.
- **Alternatives considered**: Best-effort skip (silent holes); wall-clock UTC.

## R14. First binary vs slice 10

- **Decision**: Slice 2 / first binary: DriveWal, durable ack, boot replay, disk-full, format, local gc_grace/compaction/TTL. Snapshot/PITR **library** + conformance fixtures compile; **admin backup jobs** and `010` wiring are slice 10 (`MigrateSlice10Required` / no `data_backup` in first-binary profile). Crash restore of `TWO` is required in the first binary (`016` US1).
- **Rationale**: `016` sequencing. WAL without snapshot still meets constitution XII.
- **Alternatives considered**: Full backup product in slice 2 (pulls `010`).

## R15. Metrics and jobs

- **Decision**: Increment exactly the `08` durability names (`db_wal_bytes_total`, `db_wal_fsync_total`, `db_wal_fsync_duration_seconds`, `db_checkpoint_*`, `db_dirty_*`, `db_unflushed_bytes`, `db_wal_lag_bytes`, `db_wal_replay_duration_seconds`, `db_wal_recovery_duration_seconds`, `db_recovery_*`). Additive: `db_wal_torn_total`, `db_wal_disk_full_total` (do not rename `08` names). Jobs: `compaction`, `flush`, `checkpoint`, `vacuum`, `GC`, `TTL expiration`, `snapshot`, `backup`, `data restore` — behavior here; `data_backup`/`data_restore` **orchestration** remains `010`.
- **Rationale**: Observability contract.
- **Alternatives considered**: New `wal_sync_*` names (would duplicate `db_wal_fsync_*`).

## R16. Hybrid durability

- **Decision**: Hybrid counted ack = WAL durable on the drive, covering the **write** (including the memory portion that `WriteBuffer`/`HotSet` would lose on crash). After restart, hybrid memory portion is **rebuilt** from WAL + SSTables per `003` HybridPolicy — not left empty like memory-mode.
- **Rationale**: FR-001 hybrid = durable; `003` hybrid is not volatile.
- **Alternatives considered**: Hybrid memory-only ack (rejected: would match memory-mode and violate spec).

# Data Model: Durability, WAL, Restore, Deletes, TTL, Compaction, and Backup

**Feature**: `013-durability-and-recovery` | **Date**: 2026-09-18 | **Spec**: [spec.md](spec.md)

Drive-local objects live under `{drive.path}/`. Snapshot artifacts under `{data_dir}/snapshots/`. Catalog definitions remain `003`/`006`. Validation codes are in contracts.

## 1. DriveWal

| Field | Type | Notes |
|-------|------|--------|
| drive_id | string | `004` drive name, or `default` if only `data_dir` |
| path | path | `{drive.path}/wal/` |
| next_lsn | u64 | next to assign |
| durable_lsn | u64 | last fsynced LSN |
| format_major | u16 | currently 1 |
| sync | `fdatasync` \| `fsync` | `none` not a production value |

**Invariants**: exactly one `DriveWal` per drive that hosts persistent/hybrid data. A node with one such drive has one WAL. Records for containers **not** on this drive MUST NOT appear. Group commit advances `durable_lsn` monotonically.

## 2. WalRecord

| Field | Type | Notes |
|-------|------|--------|
| lsn | u64 | per-drive |
| container_id | UUID | |
| stamp | HlcStamp | source-domain HLC (`012`); ingest time if no HLC yet on first-binary single node |
| type | `put` \| `delete` \| `txn_marker` | `delete` is a tombstone |
| payload | bytes | encrypted with container data key when the container is encrypted |

**Invariants**: CRC valid or record is torn (skip at tail only). Encrypted payload unwraps with `014` key ref; missing key → container `Unavailable{KeyUnresolvable}`, record retained.

## 3. DurableAck

| Field | Type | Notes |
|-------|------|--------|
| replica | node_id | |
| drive_id | string | |
| lsn | u64 | this replica’s drive WAL |
| kind | `durable` \| `memory` | |

**Invariants**: persistent/hybrid counted acks are `durable` only after `durable_lsn ≥ lsn`. Memory-mode counted acks are `memory` and MUST NOT be labelled crash-durable (`004` mixed-set reporting).

## 4. Checkpoint

| Field | Type | Notes |
|-------|------|--------|
| drive_id | string | |
| covered_lsn | u64 | all containers on the drive flushed through this LSN |
| at | HlcStamp | |

**Invariants**: WAL prefix `< covered_lsn` MAY truncate only if R5 retain rules pass (`gc_grace`, replica lag, snapshot pins).

## 5. Tombstone

| Field | Type | Notes |
|-------|------|--------|
| container_id | UUID | |
| key | type key | |
| created_lsn / stamp | | |
| gc_grace_until | timestamp | created + container/namespace/`storage.gc_grace` |

**Invariants**: Compaction drops only when source-domain replicas have seen it (or snapshot-rebuild) **and** `now ≥ gc_grace_until` **and** followers not needing the source-log delete. Null / missing field / tombstone remain three `003` states.

## 6. SnapshotManifest

| Field | Type | Notes |
|-------|------|--------|
| snapshot_id | UUID | |
| scope | container \| namespace | |
| positions | `{ drive_id: lsn }` | independent cuts |
| containers | `[ContainerId]` | definitions/options always; content only persistent/hybrid |
| key_refs | `[KeyRef]` | `014` |
| format_major | u16 | |

**Invariants**: no memory-mode **content**. Positions MAY correspond to different wall times. MUST NOT claim cluster-wide crash-consistency.

## 7. PitrTarget

| Field | Type | Notes |
|-------|------|--------|
| kind | `positions` \| `hlc` | |
| positions | `{ drive_id: lsn }` optional | |
| hlc | HlcStamp optional | source-domain / ingest |

**Invariants**: HLC maps per drive to max LSN with `stamp ≤ hlc`. Any unmappable involved drive → refuse. Result LSN ≤ last durable ack on that drive.

## 8. RestoreJob (mechanics; `010` stores the job row)

| Field | Type | Notes |
|-------|------|--------|
| snapshot_id | UUID | |
| pitr | PitrTarget optional | |
| destination | same cluster names \| new cluster | |
| confirm_drop | bool | required if dest has content |
| key_ref | optional | unlock / re-encrypt |

**Invariants**: dest with content + `confirm_drop=false` → refuse, dest intact. After drop/empty, fill names. Cross-namespace requires admin.

## 9. DriveDiskState

| Field | Type | Notes |
|-------|------|--------|
| drive_id | string | |
| state | `ok` \| `full` \| `corrupt` | |
| quarantined | `[path]` | |

**Invariants**: `full` → new durable writes for containers on this drive refused; other drives unaffected; `node_state=degraded`.

## State machines

### WAL group commit

`buffered → fsyncing (blocking pool) → durable (release acks) | failed (disk_full / io, no counted ack)`

### Container after boot

`Restoring → Ready` (persistent/hybrid replayed) | `RestoredEmpty` (memory-mode) | `Degraded` (quarantine, rebuild) | `Unavailable` (key / unknown format)

### Restore

`refused_has_content → (drop \| confirm_drop) → filling → ready`

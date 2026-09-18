# Contract: Per-drive WAL

**Feature**: `013-durability-and-recovery` | Crate: `crates/storage` | Spec: FR-003, FR-004, FR-005 | Research: R2–R5

## Stream

One WAL per `004` drive that holds persistent or hybrid data. Path: `{drive.path}/wal/<seq>.wal`. Implicit drive `default` = `storage.data_dir` when no `drive` blocks exist.

Segment header: magic `WAL1`, `format_major`, `drive_id`, creating node id.

## Record

See [data-model.md](../data-model.md) `WalRecord`. CRC failure at the **tail** → skip (torn); CRC failure in the **middle** → quarantine segment, container `degraded`.

Encrypted payload: container data key; AAD `drive_id ‖ lsn ‖ container_id`. Unencrypted container: plaintext payload.

## Group commit

Defaults: `max_wait=2ms`, `max_bytes=1MiB`, `sync=fdatasync`. Durability syscall on `spawn_blocking`. Counted persistent/hybrid ack waits `durable_lsn ≥ record.lsn`.

`sync none`: startup error unless `SPACESTORAGE_TEST=1`; even then acks are `memory` and MUST NOT satisfy persistent/hybrid quorum.

## Errors

| Code | When |
|------|------|
| `sync_none_not_durable` | `none` in production |
| `wal_format_unsupported` | unknown major |
| `disk_full` | ENOSPC on this drive |
| `wal_corrupt` | mid-log CRC |

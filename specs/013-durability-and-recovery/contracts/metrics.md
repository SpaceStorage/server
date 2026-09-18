# Contract: Durability metrics

**Feature**: `013-durability-and-recovery` | Crate: `storage`, `backup` | Exposition: `008`

This feature **increments** reserved `08` names. It MUST NOT rename them.

| Name | Type | Labels | Notes |
|------|------|--------|-------|
| `db_wal_bytes_total` | counter | `node`, `drive` | appended bytes |
| `db_wal_fsync_total` | counter | `node`, `drive`, `sync` | group commits |
| `db_wal_fsync_duration_seconds` | histogram | `node`, `drive` | blocking syscall |
| `db_checkpoint_total` | counter | `node`, `drive` | |
| `db_checkpoint_duration_seconds` | histogram | `node`, `drive` | |
| `db_dirty_blocks` | gauge | `node`, `drive` | |
| `db_dirty_bytes` | gauge | `node`, `drive` | |
| `db_unflushed_bytes` | gauge | `node`, `drive` | |
| `db_wal_lag_bytes` | gauge | `node`, `drive` | unflushed + untruncated |
| `db_wal_replay_duration_seconds` | histogram | `node`, `drive` | boot |
| `db_wal_recovery_duration_seconds` | histogram | `node` | |
| `db_recovery_total` | counter | `node`, `result` | `ok`, `degraded`, `format_unsupported` |
| `db_recovery_duration_seconds` | histogram | `node` | |
| `db_recovery_records_total` | counter | `node`, `drive` | replayed |

Additive (not in `08` freeze, allowed): `db_wal_torn_total`, `db_wal_disk_full_total` (labels `node`, `drive`).

Jobs: increment `spacestorage_job_*` for names in [jobs.md](jobs.md). `node_state=degraded` on disk-full or unrepaired corruption (`001`/`008` existing series).

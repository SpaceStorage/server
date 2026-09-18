# Contract: Admin / CLI

**Feature**: `013-durability-and-recovery` | Crates: `admin-proto`, `spacestorage` | Auth: `CLUSTER_ADMIN` (first binary: `001` bearer)

| Verb | Args | Slice | Effect |
|------|------|-------|--------|
| `storage` | | 2 | Per-drive WAL `durable_lsn`, lag, disk state |
| `wal` | `[DRIVE]` | 2 | Segment list, torn/quarantine counters |
| `snapshot` | `--container\|--namespace` | 10 | Create snapshot; print id + position map |
| `restore` | `SNAPSHOT [--pitr-hlc T] [--confirm-drop] [--key REF]` | 10 | Restore mechanics |
| `snapshots` | | 10 | List manifests |

First binary: `storage` / `wal` required. `snapshot`/`restore` return `BackupSlice10Required` in the first-binary profile.

HTTP: `/v1/storage`, `/v1/wal`, `/v1/snapshots` (slice 10). Errors: [wal.md](wal.md), [restore.md](restore.md), [pitr.md](pitr.md).

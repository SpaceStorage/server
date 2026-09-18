# Contract: Compaction, flush, checkpoint, vacuum, GC

**Feature**: `013-durability-and-recovery` | Crate: `crates/storage` | Spec: FR-011 | Names: `008`

Behavior owner is this feature. Strategy **defaults** come from the type catalog (`003`). Jobs run on the blocking pool; they MUST NOT occupy Tokio workers for CPU/IO bursts.

| Job (`08` name) | Effect |
|-----------------|--------|
| `flush` | memtable → SSTable; records covering LSN |
| `checkpoint` | persist covered_lsn; MAY truncate WAL prefix if retain rules pass |
| `compaction` | LSM merge; tombstone drop only per [tombstones.md](tombstones.md) |
| `vacuum` | reclaim unused pages/segments after drop |
| `GC` | file unref after compaction; respect snapshot pins |
| `TTL expiration` | emit tombstones |

Observability: `spacestorage_job_*` with those `job` label values (`008`). LSM figures `db_lsm_*` remain type/LSM increments; this feature still owns **when** compaction may drop deletes.

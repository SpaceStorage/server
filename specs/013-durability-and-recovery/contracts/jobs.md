# Contract: Background jobs this feature runs

**Feature**: `013-durability-and-recovery` | Names frozen in `008` FR-012

| `job` label | Runner crate | Slice |
|-------------|--------------|-------|
| `compaction` | `storage` | 2 |
| `flush` | `storage` | 2 |
| `checkpoint` | `storage` | 2 |
| `vacuum` | `storage` | 2 |
| `GC` | `storage` | 2 |
| `TTL expiration` | `storage` | 2 |
| `snapshot` | `backup` | 10 (library in 2) |
| `backup` | `backup` | 10 |
| `data restore` | `backup` | 10 |

`data_backup` and `data_restore` **job rows** are `010`; they invoke `backup` APIs. This feature MUST NOT register a second pair of names for the same work.

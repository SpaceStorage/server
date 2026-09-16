# Contract: Quotas

**Feature**: `007-tenancy-security` | Crate: `tenancy` | Spec: FR-002, FR-003, FR-014 | Research R4–R7, R12

## Units

| Unit | What it counts | First binary |
|------|----------------|--------------|
| `bytes` | logical stored size (one copy) | enforce only with `tenancy-quotas` |
| `objects` | logical row/object/key count (one copy) | slice 7 |
| `connections` | concurrent authenticated sessions in the namespace | slice 7 |
| `ops_per_sec` | IOPS-equivalent token bucket (1s refill) | slice 7; optional |

Replica factor MUST NOT multiply `bytes` or `objects`. Physical replica disk is `08`, not a unit.

## Caps vs usage

- **Caps** (`QuotaSpec`) live on `NamespaceRecord` (cluster log). Only `CLUSTER_ADMIN` may set, raise, or lower. `NAMESPACE_ADMIN` MAY read. `CONFIGURE` MUST NOT write caps.
- **Usage** is in-memory (R4). Not Raft-committed.
- Unset unit = unlimited (no quota reject). Limit `0` = reject consuming work immediately.
- Lower below usage: accepted; later consuming requests reject until usage falls.

## Best-effort hard reject

- Request that arrives when `current >= limit` → `QuotaExceeded { quota, limit, usage }`. MUST NOT hang.
- In-flight overlapping admits MAY briefly exceed.
- MUST NOT wait on the cluster primary solely to account quota.
- Cluster-majority loss: coordinators keep last cached caps; do not hang.

First binary: `QuotaReplace` → `Slice7Required`. Unset quotas MUST NOT reject writes.

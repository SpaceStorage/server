# Contract: PITR

**Feature**: `013-durability-and-recovery` | Crate: `crates/backup` | Spec: FR-013 | Clarify 2026-09-18 Q5

## Targets

1. **Positions**: `{ drive_id → lsn }` after the snapshot cut, `lsn ≤ durable_lsn` on that drive.
2. **HLC / ingest**: one source-domain stamp `T`. Per involved drive, `lsn* = max { lsn | record.stamp ≤ T }` (records after the snapshot cut).

## Refuse

If any involved drive cannot map `T` (no stamps, missing WAL) → `pitr_unmappable` naming the drive. **No** drive is applied past the snapshot under that failed job.

MUST NOT apply past last durable ack on a drive.

TTL event-time and operator wall clock are not PITR clocks.

## First binary

Library + tests. Operator jobs: slice 10 (`010`).

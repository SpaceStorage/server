# Contract: Backup and restore jobs

**Feature**: `010-migration-transforms` | Crate: `crates/migrate` | Spec: FR-005, FR-006 | Mechanics: `013`

## Rule

These jobs **MUST NOT** write a second snapshot format. They call `013` snapshot/PITR APIs and record ids/positions on the job.

## DataBackup

Input: `scope` = `{ "container": "…" }` or `{ "namespace": "…" }`; `pitr`: bool.

On success: job `completed`; body includes `snapshot_id` and WAL position from `13`. Encrypted containers → encrypted backup files with the **same key references** (`14`).

`08` label: `data_backup` (not `snapshot` / `backup` — those remain `13` internal jobs).

## DataRestore

Input: `snapshot_id`, optional `pitr_position`, dest cluster/namespace, optional `key_ref`.

Cross-namespace restore: same authz as cross-namespace migrate.

Missing keys → fail naming the reference (SC-005). PITR MUST NOT pass last durable ack (`13`).

Restore-in-place after a drop is `13` replace; this job only schedules it. Live dest name collision still `NameExists` unless `13` restore-replace was requested **and** `13` accepts it.

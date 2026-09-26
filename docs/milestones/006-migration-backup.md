# Milestone: migration-backup (slice 10)

**Profile**: `migration-backup`  
**Tag**: `slices-1-10`

## Implemented

Slices `1..=10` shipped. Ledger: [006-migration-backup.yaml](006-migration-backup.yaml).

Slice **10** DoD (`010-migration-transforms` + `013` snapshot/PITR):

- New crate `spacestorage-migrate` (`JobService`, dual-write, migrate/transform/backup orchestration)
- Real `spacestorage-backup` SnapshotService / PITR map / RestoreService (SNP1 manifests, confirm_drop, key_ref refuse, memory omitted)
- Admin HTTP: `/v1/jobs`, `/v1/migrate`, `/v1/transform`, `/v1/backup`, `/v1/restore`, `/v1/snapshots`
- CLI: `job` / `migrate` / `transform` / `backup` / `restore` (exit 5 = `MigrateSlice10Required` on first-binary)
- Config `jobs { enabled; catchup_concurrency; }` with first-binary refuse when enabled
- Release profile `migration-backup` (k=10); first-binary stubs remain for data_* jobs when slice10 off
- Conformance behind `--features migration-backup`

## Deferred

Still owed (`still_owed: true`):

- Slice 11 — UIs and ingest

Do **not** brand this milestone as `complete-product` (slice 11 remains).

## Changelog

- Replaced backup crate TODOs with snapshot/PITR/restore-fill library APIs
- Added migrate job lifecycle and cross-node/namespace/transform orchestration
- Wired admin + CLI surfaces; gated first-binary with named errors

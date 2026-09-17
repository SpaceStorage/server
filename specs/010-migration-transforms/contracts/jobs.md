# Contract: Jobs

**Feature**: `010-migration-transforms` | Crate: `crates/migrate` | Spec: FR-005, FR-007, FR-008 | Series: `08` `data_*`

## Job object (JSON)

```json
{
  "id": "018f…",
  "kind": "data_migration",
  "status": "running",
  "paused": false,
  "principal_id": "018e…",
  "progress": {
    "bytes_copied": 0,
    "objects_copied": 0,
    "bytes_remaining": null,
    "objects_remaining": null,
    "install_seq": null,
    "last_applied_source_seq": null,
    "last_source_seq": null
  },
  "error": null,
  "strategy": "live"
}
```

`kind` wire values MUST match `08` job labels: `data_migration`, `data_transformation`, `data_backup`, `data_restore`.

`status` wire values MUST be `starting` | `running` | `completed` | `failed` only.

## Operations

| Op | Effect |
|----|--------|
| `job.create` | Validate, persist, `starting` → runner |
| `job.status` | Read record + progress |
| `job.list` | Principal-visible jobs (own namespace / cluster admin all) |
| `job.cancel` | Source intact; target incomplete; `failed` with `cancelled` |
| `job.resume` | From `paused` or after node restart; continue at `last_applied_source_seq` |

## Errors (named)

| Code | When |
|------|------|
| `MigrateSlice10Required` | First-binary profile |
| `NameExists` | Dest / new-container name taken |
| `QuotaExceeded` | Start or cutover |
| `ConstraintUnsatisfiable` | `04` |
| `MappingQueryRequired` | Complex type, no query |
| `MappingSourceMissing` | Named column/container absent |
| `JobInProgress` | Second live window on source |
| `AuthzDenied` | Missing `MIGRATE` / admin / dual grant |
| `StateUnreadable` | Resume cannot read recorded seq |
| `KeyRefMissing` | Restore/backup encrypt |

## Resume / cancel

Interrupted jobs resume or fail named (FR-008). Cancel never swaps incomplete targets.

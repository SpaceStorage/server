# Contract: Metrics this crate increments

**Feature**: `010-migration-transforms` | Catalog: `008` background jobs | Spec: FR-007

Increment existing series only. Do **not** rename labels.

| When | Series | Labels |
|------|--------|--------|
| Job accepted | `spacestorage_job_queue_starting_total` | `job=data_*` |
| Runner active | `spacestorage_job_queue_running` | `job=data_*` |
| Success | `spacestorage_job_queue_completed_total`, `spacestorage_job_duration_seconds`, `spacestorage_job_queue_completed_duration_seconds` | `job`, `status=completed` |
| Failure / cancel | `spacestorage_job_queue_failed_total`, `spacestorage_job_errors_total` | `job`, `status=failed` |
| Resume after pause | `spacestorage_job_retries_total` | `job` |

`job` values this crate may set: `data_migration`, `data_transformation`, `data_backup`, `data_restore`.

Paused dual-write: keep `status=running` on series; `paused` lives on the job object only.

Bytes/objects copied are **job object fields**, not new Prometheus names (avoid a catalog amendment). `09` reads the object.

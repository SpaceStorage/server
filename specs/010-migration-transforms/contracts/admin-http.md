# Contract: admin-http routes for jobs

**Feature**: `010-migration-transforms` | Router: existing `admin-http` (`001`) | Auth: bearer then `014`

Additive routes. Bodies `application/json`. 1 MiB limit unchanged.

| Method | Path | Op | Success | Errors |
|--------|------|----|---------|--------|
| POST | `/v1/jobs` | `job.create` | 202 `{job}` | 401; 403 `AuthzDenied`; 422 named (quota, name, mapping, …); 501 `MigrateSlice10Required` |
| GET | `/v1/jobs` | `job.list` | 200 `{jobs:[]}` | 401 |
| GET | `/v1/jobs/{id}` | `job.status` | 200 `{job}` | 401; 404 |
| POST | `/v1/jobs/{id}/cancel` | `job.cancel` | 200 `{job}` | 401; 404; 409 `invalid_state` |
| POST | `/v1/jobs/{id}/resume` | `job.resume` | 202 `{job}` | 401; 404; 409 |

Aliases (same bodies as `job.create` with `kind` implied):

| Method | Path | Implied kind |
|--------|------|----------------|
| POST | `/v1/migrate` | `data_migration` |
| POST | `/v1/transform` | `data_transformation` |
| POST | `/v1/backup` | `data_backup` |
| POST | `/v1/restore` | `data_restore` |

Progress for `09` is `GET /v1/jobs/{id}` (no second topology schema).

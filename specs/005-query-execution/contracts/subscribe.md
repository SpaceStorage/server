# Contract: Subscribe-on-Results

**Feature**: `005-query-execution` | Spec: FR-014, SC-010 | Slice 8 / `query-distributed` | Clarify 2026-09-16 B

## Start

`QueryOptions.async_job = true` or the documented submit form below. Engine:

1. Allocates `ExecId`, records `JobState::Accepted`.
2. Returns that id without holding the original request.
3. Runs the plan in the background under the same timeout/cancel token.

PostgreSQL `LISTEN`/`NOTIFY` is **MUST NOT** (`015`). They MUST NOT be the wait path.

## One job, every client surface

The same `ExecId` is waitable here (stock clients **and** admin):

| Client | Submit | Poll / wait | Cancel |
|--------|--------|-------------|--------|
| PostgreSQL / ClickHouse SQL | `SET spacestorage.async = on` then the statement, or `SELECT spacestorage.job_submit($$…$$)` | `SELECT * FROM spacestorage.jobs WHERE id = $1`; `SELECT spacestorage.job_wait($1)` | `SELECT spacestorage.job_cancel($1)` |
| CQL | custom payload `spacestorage.async=true` | `SELECT * FROM system.spacestorage_jobs WHERE id = ?` | `SELECT spacestorage_job_cancel(?)` |
| Redis | `SS.JOB SUBMIT <command…>` | `SS.JOB GET <id>`; `SS.JOB WAIT <id>` | `SS.JOB CANCEL <id>` |
| Elasticsearch / S3 / WebDAV HTTP | `X-SpaceStorage-Async: on` | `GET /_spacestorage/jobs/{id}`; `POST /_spacestorage/jobs/{id}/wait` | `DELETE /_spacestorage/jobs/{id}` |
| Admin | n/a (jobs created above) | `GET /v1/jobs/{id}`; `GET /v1/jobs/{id}/watch` | `POST /v1/jobs/{id}/cancel` |
| CLI | — | `spacestorage jobs wait <id>` | `spacestorage jobs cancel <id>` |
| Internodes (peers only) | — | `SubscribeNotify` | `StageAbort` |

A wait on any row of this table MUST observe the same state machine. AuthZ of who may see a job is `014`; until then the creating principal (and `admin`) may wait.

## Chunks

Optional `Progress { rows, bytes }` then terminal `Done` or `Error`. MapReduce: completion only unless the plan documents chunks.

## Cancel

Any cancel surface sets the job token. Job → `Cancelled`; work stops in the timeout SLA window (FR-013).

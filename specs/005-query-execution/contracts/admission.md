# Contract: Admission and Memory

**Feature**: `005-query-execution` | Spec: FR-021, FR-022 | Config: [config-directives.md](config-directives.md)

## Limits (documented defaults)

| Knob | Default | Scope |
|------|---------|--------|
| `query.max_concurrent_per_node` | 512 | node |
| `query.max_concurrent_per_namespace` | 128 | namespace |
| `query.max_memory` | 256MiB | per query working set |
| `query.spill` | `off` | per query / plan |

`07` quotas MAY lower the namespace concurrent cap further; the stricter limit wins.

## Behaviour

- Acquire node slot then namespace slot then memory reservation before **Scheduled**.
- Failure: `admission_rejected{limit, current, max}` — no hang, no queue-forever. A metric `query_admission_rejected_total` increments (`08` error type `admission`).
- Buffer-full (`001`): treat as `admission_rejected{limit:"buffer"}`.
- Query text / result over `015` size limits: reject at Received or while producing rows; same named-error class (`limit_exceeded{what}`).

## Spill (FR-022)

When `spill on` **and** the physical plan marks a blocking operator (hash join build, sort, agg) as spillable: excess pages go to `{data_dir}/spill/{exec_id}/`. Directory removed on terminal state. Spill disk full or `data_dir` unwritable → `admission_rejected{limit:"spill_disk"}`. When `spill off`, exceeding `max_memory` rejects immediately.

Spill is not restored after process restart; in-flight queries do not survive restart (`013` restore is for user data, not exec scratch).

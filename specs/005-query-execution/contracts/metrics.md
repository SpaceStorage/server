# Contract: Query-processing metrics produced here

**Feature**: `005-query-execution` | Spec: FR-030 | Names: `008` FR-006

This crate **increments** figures; it does **not** name or drop series. `008` owns Prometheus names.

## Required increments (when the activity occurred)

| Figure | When |
|--------|------|
| query totals | every execute |
| query errors | terminal `Error` (labelled `error_type`) |
| query in flight | gauge around Scheduled→terminal |
| rows/documents/values returned | `Done` |
| result size histogram (bytes) | `Done` |
| execution / queue / wait histograms | stages |
| retries | shuffle/stage retry |
| hits / misses / lookup probes / index vs full scan | scan tasks |

## Labels (when known)

`kind` (IR variant), `namespace`, `schema`, `datatype`, `node` (coordinator), `storage_type`, drive or memory name, `error_type`.

No parallel `exec_*` public names. Tests assert the `008` names exist on `/metrics` after a smoke query (`016` first-binary: labels that exist for implemented paths).

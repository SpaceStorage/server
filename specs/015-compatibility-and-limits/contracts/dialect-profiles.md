# Contract: Dialect profiles

**Feature**: `015-compatibility-and-limits` | Crate: `crates/compat`

Never brand a profile as “v1”.

| Profile | `016` | Handlers | PG | Redis | Isolation / COPY / cursors / spill | ES aggs |
|---------|-------|----------|----|-------|-------------------------------------|---------|
| `FirstBinary` | slices 1–5 | `postgresql`, `redis` only | 3.0 simple+extended, DML/DDL auto-commit, prepared. No COPY, BEGIN, SQL DECLARE | RESP2 MUST list on `K/V Store` | No SNAPSHOT (no BEGIN). Spill off. Size/connection/admission reject | No ES handler |
| `HandlersComplete` | slice 6 | all eight | same PG subset as first binary | same Redis MUST | same | CRUD + MUST search; aggregations not-supported |
| `CompleteProduct` | slice 8+ | all eight | + BEGIN/COMMIT/ROLLBACK, COPY text/csv/binary, forward-only DECLARE/FETCH/CLOSE, EXPLAIN required | + type-specific Redis verbs documented in `002` mappings | SNAPSHOT on capable types. Spill on for sort/hash/agg | Closed aggregation list |

Entrypoint naming a handler absent from the profile → startup `unknown_handler` (`001`/`016`).

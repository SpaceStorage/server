# Contract: Canonical IR

**Feature**: `005-query-execution` | Crate: `crates/exec` | Spec: FR-001–FR-004, FR-035

## Ownership

`LogicalRequest` + `QueryOptions` is the **only** execution representation. Handlers (`002`) lower wire text/frames into it. This crate plans and executes it. A handler crate MUST NOT call `types::Container` mutators except through `QueryEngine`.

## Additive variants

Documented in [data-model.md](../data-model.md) §2. Existing `002` variants keep their meaning. `Join`/`Aggregate` execution beyond single-node nested-loop requires feature `query-distributed`.

## MUST NOT and dialect gates

| Input | IR produced | Terminal error |
|-------|-------------|----------------|
| Protocol MUST NOT verb (`015`) | none | handler not-supported |
| First-binary `COPY` / `BEGIN` (`016`) | none | handler not-supported |
| `SERIALIZABLE` | none | `NotSupported{what:SERIALIZABLE}` |
| ES agg beyond terms + metric set | none | `NotSupported{what:elasticsearch_agg}` |
| `LISTEN`/`NOTIFY` as job wait | none | handler not-supported (`015`); use `spacestorage.job_wait` |

## Equivalence (FR-003)

Two `LogicalRequest` values that differ only by protocol metadata (`QueryOptions.protocol`) MUST produce the same logical `CanonicalValue`s. Conformance: write via PostgreSQL, read via Redis (and the reverse) on first binary; broader matrix when slice 6 handlers exist.

## Batch vs txn

`Batch` without an open txn = sequential auto-commit per element, stop on first error, no rollback (`002` documented). Open txn: all elements share `Transaction.id`.

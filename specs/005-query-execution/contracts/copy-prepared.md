# Contract: Prepared Statements and COPY

**Feature**: `005-query-execution` | Spec: FR-023, FR-024

## Prepared (FR-023)

Handlers that already parse extended query / CQL PREPARE store a `LogicalRequest` with parameter slots. `EXECUTE`/`Bind` fills `CanonicalValue`s and calls `QueryEngine` (stage **Bound**). Results MUST match the equivalent ad-hoc statement (SC, Story 4).

First-binary PostgreSQL **includes** extended/prepared (`016`). CQL prepared is complete-product / slice 6.

## COPY (FR-024)

Complete-product PostgreSQL only.

| Direction | IR | Behaviour |
|-----------|-----|-----------|
| `COPY … FROM` | `CopyIn` | Row stream → `Mutate::Insert` batches; same txn if one is open |
| `COPY … TO` | `CopyOut` | `Scan` encoded as text/csv/binary |

Types that do not accept tabular data → `unsupported_by_type`. First-binary dialect: handler returns not-supported **before** IR (SC-012).

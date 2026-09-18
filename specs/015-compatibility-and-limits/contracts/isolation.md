# Contract: Isolation set

**Feature**: `015-compatibility-and-limits` | Spec: FR-006, FR-007 | Exec: `005` attaches; types: `003` flag

Closed set: `READ COMMITTED` (SQL default), `SNAPSHOT` where `TypeDescriptor.snapshot_capable`.

## Mapping (PostgreSQL / SQL)

| Client text | Stored |
|-------------|--------|
| omitted | `ReadCommitted` |
| `read committed` | `ReadCommitted` |
| `read uncommitted` | `ReadCommitted` (upgrade; session inspectable) |
| `repeatable read` | `Snapshot` |
| `snapshot` | `Snapshot` |
| `serializable` | refuse `serializable_nongoal` — not a product later |

ClickHouse: no SQL isolation SET in MUST; auto-commit unless a documented session option in `002`. Cassandra consistency is **not** isolation (`012`).

## SNAPSHOT rules

- Honoured only if **every** container the transaction touches is `snapshot_capable`.
- Otherwise refuse `snapshot_unsupported{type}` naming the type and the allowed set. **No silent downgrade.**
- First-binary types: Relational Table true, Document Store true, K/V Store false.
- New types default false.
- FirstBinary / HandlersComplete: no BEGIN → SNAPSHOT not required (BEGIN is MUST NOT).

`005` implements snapshot reads; this crate only maps and refuses.

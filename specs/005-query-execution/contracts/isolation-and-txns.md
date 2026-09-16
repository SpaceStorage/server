# Contract: Isolation and Transactions

**Feature**: `005-query-execution` | Spec: FR-015–FR-020, FR-034 | 2PC: `004` `contracts/distributed-transactions.md`

## Closed isolation set

| Requested | Result |
|-----------|--------|
| omitted (SQL) | `READ COMMITTED` |
| `READ COMMITTED` | applied |
| `SNAPSHOT` on type with `isolation.snapshot` | applied; statements share `SnapshotToken` |
| `SNAPSHOT` otherwise | `snapshot_unsupported{type}` |
| `SERIALIZABLE` (any spelling) | `NotSupported` product non-goal; allowed set named in the error |

Redis/S3/WebDAV/ES sessions are auto-commit unless a documented multi-command mapping exists (none in first binary). Cassandra consistency is **not** stored in `isolation`.

## BEGIN / COMMIT / ROLLBACK

Complete-product SQL MUST (`015`). First-binary PostgreSQL: handler not-supported (`016`) — this crate still implements the state machine for complete-product builds.

- `BEGIN` opens `Transaction` bound to **this session and protocol**.
- `COMMIT` on a single participant set: local durable commit (`013`).
- `COMMIT` when `distributed` (multi-shard or capability declared): `placement::txn` 2PC. Unreachable participant → single durable outcome or `InDoubt` visible in `spacestorage txns` within `txn_timeout` (`004` FR-077).
- `ROLLBACK`: no writes visible; buffers dropped.

Deadlock/conflict: `RetryableTxn`; no partial commit (FR-020).

## Mixed protocol

A txn is invalid across protocols. A second protocol on the same TCP connection is a new session (`002`).

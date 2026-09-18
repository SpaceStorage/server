# Contract: Crash recovery, torn tail, corruption, disk-full

**Feature**: `013-durability-and-recovery` | Crate: `crates/storage` | Spec: FR-006, FR-007 | Constitution XII

## Boot (before `ready`)

1. Catalog restore (`003`/`006`) — definitions and options of every local container.
2. For each drive: open WAL, skip torn tail, replay `lsn > checkpoint` for persistent/hybrid containers on that drive (LSN order).
3. Memory-mode: content empty; description states volatility; `004` MAY re-populate.
4. Hybrid: rebuild memory portion from WAL + SSTables per HybridPolicy.

Parallelism: drives in parallel on the blocking pool; one WAL is serial.

## Torn tail

Last record short or CRC-invalid → skip, do not advance `durable_lsn` past it, increment `db_wal_torn_total`, continue. Not a startup failure.

## Corruption

Non-tail corrupt file → `{path}.corrupt.{unix_ts}`; not used as truth; rebuild from replica or container `degraded`. No crash-loop.

## Disk-full

ENOSPC on drive D: new durable writes for containers pinned to D → `disk_full`; other drives accept writes; reads that can be served continue; `node_state=degraded`. Group-commit waiters not yet fsynced fail without a counted ack. Previously counted acks stay counted.

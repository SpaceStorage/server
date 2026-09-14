# Contract: Distributed Transactions (2PC) and Leadership Seam

**Feature**: `004-distribution-placement` | Crate: `crates/placement` (`txn.rs`) | Spec: FR-076–FR-079 | Syntax/isolation: `05`

## When it runs

Container declared `capability.distributed_transactions` (marker in `003`). A client request that spans more than one shard/participant (from `05`, or a `LogicalRequest::Batch` marked atomic) uses this commit. Single-shard writes do not.

## Protocol

Receiving node = transaction coordinator (not a sticky leader; Q1).

1. **Prepare**: `TxnPrepare` is a quorum write (container write quorum, durable filter) of a prepare record on each participant replica set.
2. Participants **Vote** commit/abort durably in the placement log.
3. Coordinator **Commit** or **Abort** likewise. Every participant reaches the same outcome; that outcome is durable (FR-076).

Timeout `txn_timeout` (default 30 s, per-group override). After deadline the txn is `in_doubt` and **visible** (`spacestorage txns`). Any node may complete recovery by reading votes (FR-077). No participant stays undecided past the bound without appearing in that list (SC-015).

## Leadership (FR-078, FR-079)

Ordinary containers stay leaderless. If the type is compatible with `leader_election` **and** the container declares it, this feature asks `06` for `LeaderFor(container)` via:

```rust
pub trait ElectionClient: Send + Sync {          // 06 implements
    async fn leader(&self, c: ContainerId) -> Option<NodeName>;
}
```

Interim: `None` always; declaring `leader_election` without `06` ⇒ `LeaderUnavailable` on write (documented). Internodes MUST NOT run Raft. Data-path reads/writes remain accepted on every node; when a leader exists, the coordinator forwards writes to it rather than becoming a second proxy for the client.

## What this is not

SQL `BEGIN`, isolation levels, retry of `Batch` as a transaction — `05`. Cross-namespace commit — `07`.

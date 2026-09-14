# Contract: Coordinator and Internode RPC

**Feature**: `004-distribution-placement` | Crates: `crates/internode`, `crates/exec` | Spec: FR-045–FR-049 | Handler: `internode`

## Any node coordinates (Clarification Q1)

Every node accepts every request. The receiving node:

1. Resolves container → replica set / shard (cluster metadata).
2. If metadata is known stale ⇒ `PlacementViewStale` (FR-047), no guess.
3. Fans out `FanoutWrite` / `FanoutRead` to replicas (leaderless: no replica is a writer-serializer).
4. Counts acknowledgements per [quorum.md](quorum.md).
5. Answers the client. Result independent of which node received the request (FR-046); only latency may differ.

Loss of the coordinator: client retries any other node. Idempotent ops are listed on the container description (FR-049). Writes carry the HLC stamp so a retry with the same stamp is applied once.

## Handler `internode`

Must be enabled (`entrypoint { handler internode; }`) or `disable internode;` (`001` rule). Own port. Optional TLS via `entrypoint.tls` path refs.

Auth: `cluster { token_file P; }` constant-time compare (interim until `07`). Failure ⇒ close.

## Frame

```text
u32le length | u8 version=1 | u16le msg_type | payload
```

JSON payload for control and small mutations. Repair/rebalance bulk: after `RepairBegin`/`RebalanceChunk` header, raw length-prefixed bytes (SSTable blocks / memtable batches), not JSON.

## Message types

| Type | Direction | Purpose |
|------|-----------|---------|
| `Heartbeat` | both | failure detector; carries HLC and digest of placement log |
| `CatalogDelta` | both | `PlacementEvent[]` since peer cursor |
| `FanoutWrite` | coord → replica | key, canonical value, stamp, txn id? |
| `FanoutRead` | coord → replica | key, stamp bound? |
| `Ack` | replica → coord | `ack_kind: durable\|memory\|read`, last_applied |
| `Hint` | coord → holder | missed writes for an unavailable replica |
| `RepairBegin/Chunk/End` | both | compare or replay |
| `RebalanceChunk` | both | add-then-remove data move |
| `TxnPrepare/Vote/Commit/Abort` | coord ↔ participant | 2PC |

Unknown `msg_type` ⇒ `Ack` error `unknown_message`; never close the cluster mesh for a version skew of one extra type.

## Heartbeat / failure

Interval `cluster.heartbeat_interval` (default 2s). Peer `unreachable` after `cluster.failure_timeout` (default 15s). Not phi-accrual (research R18). Decommission is explicit.

## Test-only delay

`internode { delay { to <peer> <duration>; } }` accepted only when `SPACESTORAGE_TEST=1`; otherwise `unknown_directive`. Used for SC-010.

## Execution record

Always includes `coordinator` (FR-048). Conformance SC-007 compares results from a replica-holding node and a node holding none.

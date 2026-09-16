# Contract: Shared-datatype aggregation and freshness

**Feature**: `008-observability` | Spec: FR-003, FR-024 | Extends [006 metrics-aggregation](../../006-control-plane/contracts/metrics-aggregation.md)

`006` owns **who** the namespace primary is and `MetricsPush`. This feature owns **names**, **last-value retention**, and **freshness series**.

## Messages (internodes, same port as `006`)

| `msg_type` | Direction | Body |
|------------|-----------|------|
| `MetricsPush` | member → namespace primary | local shared-datatype figures (`08` sample subset) |
| `MetricsSnapshot` | namespace primary → namespace secondaries | merged figures + `last_success` + per-datatype `up` precursor |

Interval: `cluster.raft.heartbeat`. Silent replica > `2 * heartbeat` ⇒ that datatype’s merge is stale (`up=0`) even if the leader process is up.

## Exposition

On a node that has a snapshot or is the merging leader:

- Merged series for shared datatypes (constitution: stored in memory on the primary; secondaries hold a copy for failover).
- `spacestorage_shared_aggregation_up{namespace,datatype}` `1` or `0`.
- `spacestorage_shared_aggregation_last_success_timestamp_seconds{namespace,datatype}`.

When `up=0`: **do not** drop merged series; **do not** add `stale` labels; **do not** change `last_success`. Local unaggregated series on every member remain and stay unlabeled.

When no snapshot has ever been received on this process: omit merged series and omit freshness for that datatype (not the same as stale).

## Primary process down

Surviving secondaries serve last `MetricsSnapshot` with `up=0`. New leader rebuilds from `MetricsPush`; `up=1` after a successful merge round. If all namespace voters are gone, remaining members still scrape **local** series only.

## Not this feature

Electing the namespace primary (`006`). Job/WAL correctness.

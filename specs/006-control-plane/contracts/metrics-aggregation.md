# Contract: Shared-datatype metric aggregation

**Feature**: `006-control-plane` | Spec: FR-013, SC-011 | Names: `008`

## Who

**Namespace primary** of the namespace that owns the datatype. Not cluster primary. Not lease holder.

## How

Members `MetricsPush` local figures each `cluster.raft.heartbeat`. Primary merges in memory. `stale=true` if a known replica is silent for `2 * heartbeat`. Local node `/metrics` always has that node's own series (`08` FR-003). Merged series appear on the primary node's exposition.

Primary down: local scrapes continue; merged view absent or marked stale until a new primary (`08` edge).

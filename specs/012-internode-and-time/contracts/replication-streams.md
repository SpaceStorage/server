# Contract: Replication streams

**Feature**: `012-internode-and-time` | Crate: `replication` | Spec: FR-017 | Policy/rate owned by [`004` failure-repair](../../004-distribution-placement/contracts/failure-repair.md)

## Source log

Per container, assigned in the source domain, durable with the WAL (`013`). Followers apply in `(epoch, position)` order on the `replication` port. Last source position wins. Catch-up after partition: replay log; if the source compacted past a lagging follower, rebuild from snapshot (`013` `gc_grace` / remote catch-up).

## Hints, repair, anti-entropy

`004` owns **when** (hinted window default 3 h, repair interval 24 h, bytes/s). This crate owns **how bytes move**:

- Hint replay: `HintReplay` on `replication`.
- Compare coordination: internodes `RepairBegin/End`.
- Compare/rebuild payload: `RepairChunk` on `replication`.

Read repair: internodes disagreement → LWW/merge then enqueue stale replica correction.

## Throttle

Honor `004` `repair_bytes_per_sec` / `rebalance_bytes_per_sec`. Backpressure per [frame.md](frame.md).

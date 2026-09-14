# Contract: Failure Detection and Repair

**Feature**: `004-distribution-placement` | Crates: `internode` (detector), `placement` (`repair.rs`) | Spec: FR-057–FR-064

## Detection

Internodes heartbeat. After `failure_timeout` the peer is `unreachable`; replicas on it become `unavailable`; container `state=degraded` with the constraint at risk named (FR-057). Interval and timeout are config, overridable per destination group (FR-073).

Requests at levels the remaining **counted** replicas can satisfy succeed; others fail with `QuorumUnsatisfiable` at execution, listing unavailable replicas (FR-058, FR-034).

## Hints

Coordinator writes missed mutations as hints on a healthy replica in the same group, retained `hinted_handoff_window` (default 3 h). Replay on return; replica stays `behind` until `last_applied` catches up; never reported `in_sync` early (FR-059).

## Compare

If the window elapsed or stamps cannot be ordered: Merkle compare of `(stamp, hash)` per key range against a healthy replica (FR-060). Difference reported (`repair.bytes`, `keys_repaired`). Background anti-entropy every `repair_interval` (default 24 h) at `repair_bytes_per_sec` (default 32 MiB/s), throttleable so it does not displace client traffic (FR-062).

## Read repair

Disagreement on a coordinated read: return LWW (or type merge), record the conflict, enqueue repair of stale replicas (FR-061, Q2).

## Partition

A side that cannot gather the required counted acks **refuses**; it does not accept locally (FR-063). On heal, LWW (or merge) converges; every resolved conflict is listed on admin `repairs` and in metrics.

## Decommission

Admin `decommission <node>`. Re-place every replica onto satisfying targets, show progress, restore declared factors (FR-064). If some container cannot be placed: `DecommissionBlocked{containers, constraints}` and the node stays `decommissioning` — never silently under-replicated. Automatic removal on timeout does not exist.

## Cluster health

`spacestorage health` / admin `health`: every degraded placement with container, constraint, failure, repair/rebalance in progress (Story 5 scenario 8).

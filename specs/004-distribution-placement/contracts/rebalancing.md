# Contract: Rebalancing

**Feature**: `004-distribution-placement` | Crate: `crates/placement` (`rebalance.rs`) | Spec: FR-065–FR-070

## Triggers

Add node, remove/decommission node, label change, RF change, constraint change, shard imbalance > 2× median. Each produces a `RebalancePlan` (FR-065).

## Plan shape

Ordered `Move`s. **Add-then-remove**: extra replica is `in_sync` before the old one is dropped, so write quorum availability never dips (SC-013). Anti-affinity holds after every prefix, or a step carries `temporary_violation{constraint, max_duration}` declared up front (FR-066).

Inspect: moved / in flight / remaining / rate / ETA (FR-068). Rate `rebalance_bytes_per_sec` (default 64 MiB/s per node), changeable live. `pause` / `resume` do not corrupt an in-flight chunk; paused plans stay valid.

## Resume

`plan_id` + `cursor` in the placement log. Any node may resume after coordinator death (FR-068). No restart from zero.

## Integrity

Keys resolve to exactly one location throughout (FR-069). No acknowledged data lost or duplicated. On `done`: every affected container `satisfied`, replicas `in_sync`, no residual copies outside the placement (FR-070, SC-013).

## Blocked decommission

No satisfying target ⇒ plan `blocked`, containers named. Operator must relax a constraint, add capacity, or cancel.

## Relabel

`az` correction re-evaluates anti-affinity; violations are degraded + a repair plan under these same rules (Story 8 scenario 5).

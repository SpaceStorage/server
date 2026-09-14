# Contract: Placement Planner and Director

**Feature**: `004-distribution-placement` | Crate: `crates/placement` | Spec: FR-016–FR-027 | Replaces interim `LocalDirector` when internodes is enabled

## `PlacementDirector` (additive on `003`)

```rust
pub trait PlacementDirector: Send + Sync {
    fn defaults(&self) -> PlacementDefaults;                    // 003
    async fn declare(&self, c: ContainerId, decls: &[CapabilityDecl]) -> Result<PlacementPlan, PlacementError>;
    async fn change(&self, c: ContainerId, delta: &[CapabilityDecl]) -> Result<PlacementPlan, PlacementError>;
    async fn release(&self, c: ContainerId) -> Result<(), PlacementError>;
    fn info(&self) -> &dyn PlacementInfo;                       // 002

    // 004 additions (default bodies: LocalDirector behaviour)
    fn topology(&self) -> TopologyView { TopologyView::single_local() }
    async fn reevaluate(&self, c: ContainerId) -> Result<PlacementPlan, PlacementError> { self.declare(c, &[]).await }
    fn report(&self, c: ContainerId) -> Option<PlacementReport> { None }
}
```

`ClusterDirector` is the default when an `internode` entrypoint is enabled. `LocalDirector` remains for RF=1 / tests. RF>1 or any anti-affinity declaration without internodes ⇒ `InternodesRequired`.

## Defaults (no capability declared)

RF=1, no anti-affinity, place on the local node if it satisfies storage mode, otherwise the least-loaded live node. Description names these defaults as reported by the director (FR-027, `003` FR-034).

## Planner (FR-016, FR-017)

Inputs: topology snapshot, constraint, existing placements, estimated footprint. Steps (research R4):

1. Exclude by selector, missing anti-affinity key, media/memory capacity, `unhealthy_clock`, `decommissioning`, unknown type on that node.
2. Sort remaining by (distinctness gain, load, `node_name`).
3. Greedy pick until `factor` (or per-group factors) filled.
4. If short ⇒ `PlacementUnsatisfiable` **before** allocating storage (Q5, FR-021). Never create a degraded container at declare time.

Load: `used_bytes/capacity` then request-rate EMA. Ties: lexicographic `node_name`. No randomness.

## Anti-affinity

Ordered keys e.g. `[region, az, rack]`: maximise distinct `region`, then `az` within region, then `rack` (FR-020). Required distinct values at the first key = `min(factor, domain.cardinality)`. Shortage ⇒ refuse, listing required vs available (Story 2 scenario 2). Best-effort spread is out of scope.

After-the-fact violation (relabel, death): `state=degraded`, keep serving, repair via rebalance (FR-026) — recovery, not a creation downgrade.

Raising RF: add replicas under the same constraint, populate in background. Lowering RF: remove only after the remainder satisfies the constraint (FR-025).

## Placement report

Per container, readable from any node:

- each replica: node, labels, drive or memory, group, mode, health, lag, `durable`
- exclusions with reasons
- `state`: satisfied | degraded | unplaceable
- last change reason and HLC

Identical on every node (FR-018, Story 2 scenario 8).

## `PlacementInfo` live

```text
replicas(c)     = ReplicaSet.n  (not always 1)
satisfiable(q,c)= required(q,c) <= counted_available(c, write/read)
clamp(q,c)      = highest satisfiable ≤ q
```

Counted availability for **writes** on persistent/hybrid uses only `durable` replicas (Q3).

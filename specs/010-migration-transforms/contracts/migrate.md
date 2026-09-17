# Contract: Migration

**Feature**: `010-migration-transforms` | Crate: `crates/migrate` | Spec: FR-001, FR-002, FR-009, FR-010, FR-012

## Strategies

| Name | Default? | Behavior |
|------|----------|----------|
| `live` | yes (node-to-node) | Dual-write + catch-up + cutover |
| `snapshot` | disaster | Invoke `13` snapshot of source, restore to dest |
| `offline` | no | Pause source writes until copy completes |

## Node / drive

Request names source container and dest node ids and/or drive labels. `PlacementDirector` (`04`) validates anti-affinity and RF **before start and before cutover**. Replica streaming remains `04`.

Subset of replicas: `replica_slots[]` optional; omitted = move every replica the dest policy requires.

## Namespace

`policy: copy` — dest created, source remains.  
`policy: move` — after cutover, **drop** source (name reusable).

Cross-namespace: `CLUSTER_ADMIN` or `MIGRATE` on **both** namespace ids.

Dest name default = source name. Collision → `NameExists` (no overwrite/merge).

## Quota

Logical size (`07`). Start: dest must fit source size **as a second copy**. Cutover: re-check; fail named; source intact; dest incomplete.

## No-op

Same namespace, copy, no dest nodes/drives → refuse `NoOp`.

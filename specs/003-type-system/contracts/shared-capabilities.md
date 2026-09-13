# Contract: Shared Capabilities (L1) and the Placement Seam

**Feature**: `003-type-system` (declaration + compatibility) | Crate: `crates/placement` | Future owner of semantics: feature `04-distribution-placement` | Spec: FR-004, FR-008, FR-030–FR-036

L1 is a **capability layer over types**, never a separate database (FR-030, Constitution IV). This feature owns *what may be declared on which type* and *validation before placement*; `04` owns *what a declaration does*.

## 1. Capability registry (13, expandable)

| Capability | Declarable parameters | Typically fixed after create |
|---|---|---|
| `replication` | `factor: u16`, `anti_affinity: LabelKey?`, `mode: sync \| async` | — (factor and anti-affinity are changeable) |
| `sharding` | `key: [FieldPath]`, `shards: u32?` | `key` |
| `partitioning` | `scheme: range \| hash \| time`, `key: [FieldPath]`, `interval: Duration?` | `scheme`, `key` |
| `node_placement` | `labels: LabelSelector` | — |
| `persistent_placement` | `labels: LabelSelector` (disk class, e.g. `disk in (nvme, ssd)`) | — |
| `labels` | `labels: LabelSelector` (general placement constraints) | — |
| `consistency` | `mode: strict \| eventual \| bounded(window)` | — |
| `quorum` | `write: QuorumLevel`, `read: QuorumLevel` | — |
| `failure_handling` | *(marker)* | — |
| `rebalancing` | *(marker)* | — |
| `distributed_transactions` | *(marker)* | — |
| `consensus` | *(marker)* | — |
| `leader_election` | *(marker)* | — |

Marker capabilities declare participation; their behaviour is `04`/`06`. `quorum` here only records per-container defaults — per-query quorum stays on the query (`002`, `05`, Constitution VIII).

## 2. Compatibility matrix

Each `TypeDescriptor` carries one row:

```rust
pub enum CapabilitySupport {
    Unsupported,
    Supported { declarable: &'static [ParamName], fixed_after_create: &'static [ParamName] },
}
```

Baseline (full matrix generated into `docs/types/<name>.md`):

| Type group | replication | sharding | partitioning | placement / labels | consistency / quorum | dist. txn | consensus / leader |
|---|---|---|---|---|---|---|---|
| L0 data structures (memory-oriented: `tuple`, `vector`, `linked_list`, `deque`, `ring_buffer`, `heap`) | ✔ | ✖ | ✖ | ✔ | ✔ | ✖ | ✖ |
| L0 ordered/indexed (`bplus_tree`, `skip_list`, `radix_tree`, `hash_table`, `bitmap`, `bloom_filter`) | ✔ | ✔ | ✔ | ✔ | ✔ | ✔ | ✖ |
| L0 ANN (`kd_tree`, `hnsw`, `scann`) | ✔ | ✔ (by id) | ✖ | ✔ | ✔ | ✖ | ✖ |
| L2 abstractions | ✔ | ✔ | ✔ (keyed types) | ✔ | ✔ | ✔ | ✖ |
| L3 models | ✔ | ✔ | ✔ | ✔ | ✔ | ✔ | ✔ (`log_stream`, `timeseries`) |
| L4 compositions | ✔ (of the composition object) | ✖ | ✖ | ✔ | ✔ | ✔ | ✖ |

A capability registered later is `Unsupported` for every existing type until that type's descriptor says otherwise (FR-031, Story 4 scenario 7).

## 3. Declaration and validation

Declared with the flat options of [container-definition.md](container-definition.md):

```text
capability.replication.factor=3
capability.replication.anti_affinity=az
capability.sharding.key=customer_id
capability.persistent_placement.labels=disk in (nvme)
```

Validation (stage 9, before anything reaches placement — FR-032):

| Code | Rule |
|---|---|
| `CapabilityUnsupported{type, capability, supported}` | the type's row says `Unsupported` |
| `CapabilityParamUnknown{capability, param}` | parameter not declarable |
| `CapabilityParamFixed{capability, param}` | change to a `fixed_after_create` parameter (FR-035) |
| `CapabilityConflict{a, b}` | mutually inconsistent declarations — e.g. a partitioning key not contained in the sharding key when the type requires containment; `replication.mode=async` with `consistency.mode=strict` |
| `CapabilityFieldUnknown{capability, field}` | a key or label field absent from the schema |

Declaration form is identical at every level (FR-030): the same option names on an L0 `bplus_tree`, an L2 `ordered_map`, an L3 `relational_table` and an L4 `union`.

## 4. Container-unit rule (FR-033)

Capabilities apply to the container as one unit. All layout components are placed, replicated and recovered together. `LayoutComponent` has no capability field, so a per-component placement is unrepresentable rather than merely refused.

On an L4 composition, declarations govern **the composition object itself** (its metadata and, for a materialized view, its view state); members keep their own declarations, and the description shows both so no override is hidden (spec edge case).

## 5. Defaults (FR-034)

A container with no declaration is described as *using the placement layer's defaults*, and the description names those defaults as reported by the director — never as "none". With the interim director that is: replicas 1, no anti-affinity, node-local placement, quorum from `query_defaults` (`002`).

## 6. Placement seam

```rust
pub trait PlacementDirector: Send + Sync {                    // 04 implements
    fn defaults(&self) -> PlacementDefaults;
    async fn declare(&self, c: ContainerId, decls: &[CapabilityDecl]) -> Result<PlacementPlan, PlacementError>;
    async fn change(&self, c: ContainerId, delta: &[CapabilityDecl]) -> Result<PlacementPlan, PlacementError>;
    async fn release(&self, c: ContainerId) -> Result<(), PlacementError>;
    fn info(&self) -> &dyn PlacementInfo;                     // bridges to 002's exec seam
}
```

Interim `LocalDirector`: `replicas() == 1`, every declaration accepted after matrix validation and recorded, plan = "this node". It implements `002`'s `PlacementInfo` so quorum satisfiability, clamping and `05`'s routing keep working unchanged.

## 7. What this contract does **not** define

How replicas are placed across racks, AZs, regions or planets; what a consistency mode or quorum level means in terms of acknowledgements; how failures are detected and rebalanced; how distributed transactions, consensus and leader election operate. All of that is `04` (and `06` for controllers) — this feature only carries typed, validated declarations to them (FR-036).

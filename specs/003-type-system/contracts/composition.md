# Contract: L4 Storage Composition

**Feature**: `003-type-system` | Crate: `crates/l4` | Spec: FR-007, FR-008, FR-008a, FR-018, FR-019 | Clarification Q3

An L4 composition is a container like any other: it has a name, a type, a namespace, a description, and it is listed by every protocol with its type name (FR-020).

## 1. Definition

```text
compose.kind      = union | federated | materialized_view | distributed | partitioned | replicated | sharded
compose.members   = <container name>[,<container name>…]     # resolved to ContainerIds at definition time
compose.rule.*    = kind-specific (below)
compose.refresh.* = materialized_view only
```

Members are stored as `ContainerId`s, never names, so renaming a member leaves every composition working (spec edge case).

## 2. Kinds

| Kind | Rule options | Read | Write (Clarification Q3) |
|---|---|---|---|
| `union` | `rule.order=<member order>` | rows/items of all members in member order | **read-only** — `CompositionReadOnly{union}` naming the members |
| `federated` | `rule.route=<predicate over key or field>` | one logical object over heterogeneous members | routed to the single member the predicate resolves; zero or several ⇒ `CompositionWriteAmbiguous{resolved}`, nothing stored |
| `materialized_view` | `rule.sources=…`, `rule.query=<canonical query document>` | the materialized state | **read-only** — changes only by refresh |
| `distributed` | `rule.placement=<label selector>` | union of members by placement | routed by the placement rule |
| `partitioned` | `rule.scheme=range\|hash\|time`, `rule.key=…`, `rule.bounds=…` | union across partitions | routed to the owning partition |
| `replicated` | `rule.targets=<label selector or member list>` | any live replica | routed to the replica set |
| `sharded` | `rule.key=…`, `rule.shards=N` | union across shards | routed to the owning shard |

`distributed`, `partitioned`, `replicated` and `sharded` declare in their descriptors which L1 capabilities they compose — `node_placement`/`labels`, `partitioning`, `replication`, `sharding` — and add no placement capability of their own (FR-008). The placement itself is executed by `PlacementDirector` ([shared-capabilities.md](shared-capabilities.md)).

**No composition provides atomicity across members.** A client needing it uses the L1 `distributed_transactions` capability; the composition answers `NotSupportedByType{kind, "multi_member_atomic_write"}` with that pointer (FR-008a).

## 3. Validation

| Code | Rule |
|---|---|
| `CompositionCycle{path}` | a member set that reaches the composition itself, directly or transitively |
| `CompositionCrossNamespace{member}` | all members in the composition's namespace (FR-018) |
| `CompositionDepthExceeded{limit}` | nesting depth ≤ **8** (documented in the catalog) |
| `CompositionMemberCount{kind, min}` | `union`, `federated`, `sharded` need ≥ 2; others ≥ 1 |
| `CompositionUnionIncompatible{a, b}` | member pair not in the union-compatibility table ([type-inventory.md](type-inventory.md)) — the error points at `federated` |
| `CompositionRuleInvalid{kind, reason}` | missing or malformed kind-specific rule |

Nesting is validated at every level (`materialized_view` over a `union` over two tables is three levels).

## 4. Operation set

A composition's operation set is **computed** from its members' sets: the intersection of read operations the kind can fan out or route, plus the kind's own (`refresh`, `add_member`, `remove_member`, `describe`). It is shown in the description and is what the planner and drivers see (Story 5 scenario 2). An operation no member supports is `NotSupportedByType`.

## 5. Materialized view refresh

```text
refresh.policy = sync | async
refresh.interval = <duration>        # async only
refresh.on = commit | interval       # async only
```

- `sync`: the source write completes after the view is updated; a view failure fails the source write.
- `async`: the view is refreshed on the interval or on source commit; the description carries `last_refresh`, `staleness` and `pending`.
- Source unavailable (all replicas down): the view keeps serving its last refreshed state, reports staleness, and a refresh attempt reports the source's unavailability rather than clearing the view (spec edge case).

## 6. Missing members and drops

| Kind | Missing-member policy |
|---|---|
| `union`, `federated` | continue with the remaining members; record the removal in the description; state `Degraded{missing}` |
| `partitioned`, `sharded`, `distributed`, `replicated` | refuse operations addressing the missing range/shard/target; other ranges keep working |
| `materialized_view` | serve last refreshed state, report staleness |

Dropping a member without cascade is refused with the dependants listed (FR-019); with cascade, each dependent composition is dropped or updated per the table above, and the update is recorded.

## 7. Capabilities on a composition

Validated against the composition kind's own matrix row (FR-032). They govern the composition object — its catalog record and, for a view, its materialized state — while members keep their own declarations. The description shows both (spec edge case).

## 8. Test obligations (SC-009)

Every kind created over members of at least two types (or two members of one type for `union`); listed with its type name through every protocol; cyclic and cross-namespace definitions refused; write rules asserted per kind (read-only refusal, single-member routing, ambiguous-write refusal with nothing stored); member drop with and without cascade; nesting to depth 8 accepted and depth 9 refused; view refresh under both policies including a stale-source case.

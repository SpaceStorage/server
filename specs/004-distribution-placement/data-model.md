# Data Model: Distribution and Placement

**Feature**: `004-distribution-placement` | **Spec**: [spec.md](spec.md) | **Plan**: [plan.md](plan.md)

Entities the placement engine stores and exposes. User data remains in `003` containers; this model is cluster metadata and replica-set state.

## 1. Node

| Field | Type | Notes |
|-------|------|-------|
| `name` | `NodeName` | Unique in the cluster; config `node.name` |
| `labels` | `BTreeMap<LabelKey, LabelValue>` | Operator-declared |
| `derived_labels` | `BTreeMap<LabelKey, LabelValue>` | From drives (`media_nvme=true`, …); distinguished in the topology view |
| `drives` | `[Drive]` | May be empty on a memory-only node |
| `memory` | `Option<MemoryPool>` | Absent ⇒ excluded from memory placements |
| `status` | `joining \| live \| unreachable \| decommissioning \| removed` | |
| `clock_skew` | `Duration` | vs coordinator HLC; `unhealthy_clock` when > `max_stamp_skew` |
| `peers_seen_at` | `Timestamp` | last internodes heartbeat |

**Invariants**: no duplicate label keys; `name` collision refused (`node_name_conflict`). Live mutations of labels/drives/memory emit `PlacementEvent::Topology`.

## 2. Drive

| Field | Type | Notes |
|-------|------|-------|
| `name` | IDENT | Unique per node |
| `path` | PATH | Must be writable |
| `media` | `nvme \| ssd \| hdd \| other(IDENT)` | Expandable |
| `capacity_bytes` | u64 | From `size` or `statvfs` |
| `used_bytes` | u64 | Sum of replica footprints |
| `labels` | map | Own labels; also enrich the node |

**Validation**: `drive_media_required`, `drive_path_unreadable`, `drive_name_duplicate`.

## 3. MemoryPool

| Field | Type | Notes |
|-------|------|-------|
| `size_bytes` | u64 | Config `memory.size` |
| `used_bytes` | u64 | |
| `labels` | map | |

**Validation**: `memory_size_required`, `memory_exceeds_machine` (refuse if > reported RAM).

## 4. PlacementDomain

Per `LabelKey`: `{ key, values: {LabelValue → [NodeName]}, cardinality }`. Cardinality is the max RF with anti-affinity on that key (FR-007).

## 5. LabelSelector

Parsed AST of [label-selector.md](contracts/label-selector.md). Evaluated against a `PlacementTarget` = (node labels ∪ derived labels, one drive or the memory pool). Match is boolean; capacity is a separate check.

## 6. PlacementConstraint (on a container)

Union of:

- `selector: LabelSelector` (`capability.labels` / `node_placement`)
- `media: MediaKind` or `memory` (`persistent_placement` / media sugar)
- `anti_affinity: [LabelKey]` ordered
- `persistent: bool` + pin selector (FR-014)
- `groups: [DestinationGroup]` or a single `factor: u16`

All must hold for every replica (FR-015). Unsatisfiable at declare ⇒ `PlacementUnsatisfiable` (Q5).

## 7. DestinationGroup

| Field | Type | Notes |
|-------|------|-------|
| `id` | `GroupId` | |
| `selector` | LabelSelector | |
| `factor` | u16 | replicas in this group |
| `mode` | `sync \| async` | |
| `lag_threshold` | `{ duration, writes }` | async only |
| `each_quorum_policy` | `refuse \| wait` | default `refuse` (Q4) |
| `hinted_handoff_window` | Duration | inherit cluster default |

## 8. Placement / PlacementReport

Recorded decision for one container (or shard/partition):

| Field | Type |
|-------|------|
| `container` | `ContainerId` |
| `targets` | `[Replica]` |
| `satisfied` | `bool` |
| `state` | `satisfied \| degraded \| unplaceable` |
| `exclusions` | `[(NodeName, Reason)]` |
| `created_at` / `updated_at` | HLC |
| `reason` | last change (create, rebalance, repair, label-change) |

`Reason`: `selector_miss`, `missing_anti_affinity_key`, `no_media_capacity`, `no_memory_pool`, `would_break_anti_affinity`, `type_unknown_on_node`, `clock_unhealthy`, `decommissioning`.

## 9. Replica

| Field | Type | Notes |
|-------|------|-------|
| `id` | `ReplicaId` | |
| `node` | `NodeName` | |
| `target` | `DriveName \| Memory` | |
| `group` | `GroupId` | |
| `mode` | `sync \| async` | copied from group |
| `health` | see state machine | |
| `last_applied` | Stamp | |
| `lag` | `{ duration, writes }` | async |
| `durable` | bool | drive-backed vs memory; used by write-quorum filter (Q3) |

### Replica health

```text
empty ──populate──► in_sync ──miss_heartbeat──► unavailable
                      │                              │
                      │ hint/compare                 │
                      ▼                              ▼
                   behind ◄──────── replay/compare ──┘
                      │
                      └──caught_up──► in_sync

unavailable ──decommission──► removed
in_sync ──rebalance_out──► moving ──► removed
```

A memory-mode replica after local restart is `empty` then `behind` while repopulating (type system: content did not survive). If no live replica remains: `content_lost` (reported, not `in_sync`).

## 10. ReplicaSet

Replicas of one container, shard, or partition. Quorum is computed here. `n = replicas.len()` excluding `removed`.

## 11. VersionStamp (HLC)

`(physical_micros: u64, logical: u32, node_id: NodeId)`. Total order. Conflict merge: default LWW (greater stamp wins); optional `ConflictMerge` from the type descriptor.

## 12. QuorumDecision

| Field | Type |
|-------|------|
| `requested` | `QuorumLevel` |
| `source` | `query \| session \| container \| namespace \| global` |
| `clamped_from` | `Option<QuorumLevel>` |
| `required` | u16 |
| `durable_required` | bool (writes on persistent/hybrid) |
| `contacted` | `[NodeName]` |
| `acks` | `[(NodeName, ack_kind: durable\|memory\|read)]` |
| `achieved` | u16 counted |
| `coordinator` | `NodeName` |

Written into `002` `ExecutionRecord.applied` (extended, not replaced).

## 13. ShardMap / PartitionMap

**ShardMap**: `{ scheme: hash, shards: u32, key: [FieldPath], assignments: [(ShardId, ReplicaSetId, token)] }`. Routing: rendezvous hash of canonical key → exactly one `ShardId`.

**PartitionMap**: `{ scheme: range\|time, key, splits: [(lo, hi, ReplicaSetId)] }` or `{ interval, field, buckets: [(start, ReplicaSetId, selector_override)] }`.

Invariant: every key maps to one shard and one partition at any cursor of a split (FR-052).

## 14. Hint / RepairJob

**Hint**: `{ for_replica, stored_on, writes: [FanoutWrite], expires_at }`.

**RepairJob**: `{ kind: replay\|compare, source, target, bytes_done, bytes_total, rate, state: running\|paused\|done\|failed }`.

## 15. RebalancePlan

| Field | Type |
|-------|------|
| `id` | `PlanId` |
| `steps` | `[Move]` |
| `cursor` | usize |
| `state` | `planned \| running \| paused \| blocked \| done \| failed` |
| `rate_bytes_per_sec` | u64 |
| `temporary_violations` | `[(step_index, constraint, max_duration)]` |

`Move`: `{ what: ReplicaId\|ShardId, from, to, bytes }`. Add-then-remove encoded as two steps.

Blocked: no target satisfies some container (`DecommissionBlocked{containers}`).

## 16. Transaction (2PC)

| Field | Type |
|-------|------|
| `id` | `TxnId` |
| `coordinator` | `NodeName` |
| `participants` | `[ReplicaSetId]` |
| `state` | `running \| prepared \| committed \| aborted \| in_doubt` |
| `deadline` | Timestamp |
| `stamp` | VersionStamp |

Any node may recover `in_doubt` after `txn_timeout` by reading participant votes from the log.

## 17. ClusterStore event log

```text
PlacementEvent =
  | NodeJoin / NodeLeave / LabelChange / DriveChange / MemoryChange
  | PlacementPut / PlacementDelete
  | ReplicaHealth
  | CatalogDelta applied
  | RebalanceStep
  | RepairProgress
  | TxnPrepare / TxnVote / TxnCommit / TxnAbort
```

Each event has `id: uuid v7`, `stamp: HLC`, `origin: NodeName`. Idempotent apply by `id`.

## 18. InternodePeer

| Field | Type |
|-------|------|
| `name` | `NodeName` |
| `address` / `port` | internodes entrypoint |
| `status` | `live \| unreachable` |
| `last_rtt` | Duration |
| `injected_delay` | Duration | test-only |

## Relationships

```text
Cluster 1──* Node 1──* Drive
              └─0..1 MemoryPool
Container 1──* DestinationGroup 1──* Replica ──1 Node, 1 (Drive|Memory)
Container 0..1 ShardMap 1──* ReplicaSet
Container 0..1 PartitionMap 1──* ReplicaSet
ReplicaSet 1──* Replica
Coordinator (any Node) ── Fanout ── ReplicaSet
Txn 1──* ReplicaSet (participants)
RebalancePlan 1──* Move ── Replica|Shard
```

`003` `ContainerDefinition` holds `CapabilityDecl[]`; this model is the *execution* of those decls. Incompatible decls never appear here.

## Validation codes (placement)

| Code | When |
|------|------|
| `PlacementUnsatisfiable{constraint, required, available, exclusions}` | FR-013, FR-021, Q5 |
| `AntiAffinityKeyMissing{key, nodes}` | FR-022 |
| `InternodesRequired{factor}` | RF>1 without internodes handler |
| `QuorumUnsatisfiable{requested, available}` | explicit, from `002` |
| `QuorumRequiresAsyncGroup{level, group}` | Q4 |
| `QuorumNotDurable{needed, durable_acks}` | Q3 timeout/fail |
| `EachQuorumRefused{container}` | Q4 default |
| `ShardKeyUnknown{field}` | FR-055 |
| `DecommissionBlocked{containers}` | FR-064 |
| `SelectorSyntax{line, col}` | parse |
| `DriveMediaRequired`, `MemorySizeRequired`, `NodeNameConflict`, `PeerUnknown` | topology |

Refusal body always includes container, constraint/level, topology facts, and what would resolve it (FR-082).

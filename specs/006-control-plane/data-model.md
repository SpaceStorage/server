# Data Model: Control-Plane Hierarchy, Raft Elections, and Node Restore

**Feature**: `006-control-plane` | **Date**: 2026-09-16 | **Spec**: [spec.md](spec.md)

Applied state is the `003` catalog + `004` `ClusterStore` snapshot. This document is the Raft-facing model. Validation codes are in contracts.

## 1. GroupId

| Field | Type |
|-------|------|
| kind | `cluster` \| `namespace` |
| namespace_id | UUID, present iff namespace |

On disk: `cluster` or `ns/<uuid>`.

## 2. VoterSet

| Field | Type |
|-------|------|
| group | GroupId |
| voters | odd-sized set of `NodeId` |
| learners | set of `NodeId` (disjoint from voters) |
| epoch | u64 (config change count) |

**Invariants**: `voters.len() % 2 == 1`; `voters ⊆ members`; `learners ∩ voters = ∅`; non-members neither vote nor learn as a replica target (`FR-010`).

**First binary**: size 1 after bootstrap; size 3 after third member; no replace/grow/shrink API.

## 3. RaftRole

`Follower | Candidate | Leader` among **voters**. Product names: Leader = **primary**, other voters = **secondaries**. Learners have no RaftRole. A partitioned minority of `voters` cannot commit.

## 4. ClusterState (cluster log apply)

| Field | Type |
|-------|------|
| cluster_uuid | UUID (`011`, immutable) |
| cluster_name | string (label only) |
| members | map `NodeId → MemberRecord` |
| namespaces | map `NamespaceId → { name, group_id }` |
| drives / memory | inventory as published by members (`004` topology) |
| roles | opaque blob (`07`/`14`) |
| voter_set | VoterSet for `GroupId::Cluster` |
| exclusive_data | bool, default false (slice 7) |
| hlc | last applied HLC stamp |

### MemberRecord

| Field | Type |
|-------|------|
| node_id | stable id (`011`) |
| node_name | unique in cluster |
| addresses | internodes / replication / tenant |
| labels | topology ladder keys |
| status | `joining \| ready \| draining \| dead` |

Join/leave/replace **procedures** remain `011`; this model stores the resulting membership **before** the node is added to any VoterSet.

## 5. NamespaceState (namespace log apply)

| Field | Type |
|-------|------|
| namespace_id | UUID |
| schemas | map name → SchemaDef (`003`) |
| containers | map id → ContainerDefinition (`003`, options, shared-datatype metadata) |
| leases | map `ContainerId → LeadershipLease` |
| voter_set | VoterSet for this group |
| hlc | last applied HLC stamp |

## 6. LeadershipLease

| Field | Type |
|-------|------|
| container_id | UUID |
| holder | NodeId |
| epoch | u64, monotonic per container |
| granted_index | Raft log index |

**Invariants**: only types that require single-writer/total-order have a row; leaderless types have none. Appends must present `epoch == current`. Stale epoch → `StaleEpoch`; not applied; not in ordered history.

## 7. RaftLogRecord

```text
RaftLogRecord {
  group: GroupId,
  term: u64,
  index: u64,
  hlc: Hlc,                 # FR-015
  body: ClusterOp | NamespaceOp | MembershipOp
}
```

`MembershipOp`: `Replace { from, to } | Grow { add: [NodeId; 2] } | Shrink { remove: [NodeId; 2] } | ExpandToThree { b, c }` (first-binary 1→3).

Durable on a voter majority before commit. Format version on segment files.

## 8. ControllerView (admin)

| Field | Type |
|-------|------|
| group | GroupId |
| primary | NodeId |
| secondaries | [NodeId] |
| learners | [NodeId] |
| commit_index | u64 |
| term | u64 |

## 9. SharedMetricAggregate (in-memory, not Raft)

| Field | Type |
|-------|------|
| namespace_id | UUID |
| by_datatype | map id → merged figures (`08` shapes) |
| stale | bool |
| primary | NodeId (this process iff namespace leader) |

Lost on crash; rebuilt by `MetricsPush` after election. Not a restore obligation.

## 10. RestorePlan (boot, not persisted)

Ordered steps: open local Raft dirs → apply → `013` content restore for persistent/hybrid → memory content empty → internodes up → learner catch-up → replicated memory re-populate (`004`). Volatility notice from type/storage mode.

## Relationships

```text
ClusterState 1──* MemberRecord
ClusterState 1──* Namespace (list only)
NamespaceState 1──* Schema
NamespaceState 1──* ContainerDefinition
NamespaceState 0..* LeadershipLease
Group 1──1 VoterSet
VoterSet *── NodeId (voters, learners)
Node 1──* local Raft dirs (groups it votes or learns)
Namespace primary 1──1 SharedMetricAggregate
```

## Validation codes

| Code | When |
|------|------|
| `NotLeader { group, leader }` | write on follower; forward or redirect |
| `Minority { group }` | no voter majority |
| `NotMember` | process not in membership; cannot vote or take new replicas |
| `VoterSetOdd { size }` | proposed set even |
| `VoterSetMajorityLost` | proposed change would not keep a majority at an accepted step |
| `StaleEpoch { have, need }` | ordered append with old lease |
| `LeaseNotGranted { container }` | ordered write without a lease |
| `LeaseForbidden { type }` | leaderless type requested a lease |
| `ExclusiveDataBlocked { node, containers }` | exclusive-data on while tenant replicas remain |
| `Slice7Required { op }` | migrate / exclusive on in first binary |
| `UnknownRaftFormat { version }` | refuse start (`015`) |

Refusal names group, nodes, and what would succeed (align with `004` FR-082 style).

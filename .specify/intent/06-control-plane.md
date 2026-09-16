---
speckit_command: specify
suggested_slug: control-plane
source: server/start
read_after: 00-constitution.md
---

# Feature: Control-plane hierarchy, Raft elections, and node restore

Specify the levels of subordination (primaries and secondaries) that own cluster metadata and how nodes recover.

## What

Levels of subordination:

- **Cluster-level primary** — contains all namespaces and nodes and drives (membership view from `11`).
- **Namespace-level primary** — contains all schemas and container definitions inside them.
- **Datatype-level primary** — contains **metadata and leadership** of shared datatypes (not per-write serialization; data path is leaderless, `12`).
- **Node-level** — stores state of all local datatypes in drives or memory according to the restore rules below.

All primaries MUST have their own **secondary servers** for failover and load balancing.

Different levels of controllers MUST implement **elections using RAFT**. A partitioned minority MUST NOT accept membership or schema changes (controllers are CP). Data requests remain tunable-AP via quorum (`04`/`12`).

When a node comes up it MUST restore **definitions and options** of all local datatypes. It MUST restore **content** of persistent and hybrid datatypes from drives (`13`). Memory-mode content MUST come back empty unless replication re-populates it. "All data and their state" means this split, not that RAM contents survive a crash.

Membership join/leave/replace (`11`) is recorded in cluster-level controller storage before the node votes or becomes a replica target.

Shared datatype metrics are aggregated by the **primary server** and stored in memory (series catalog is in `08`; this feature owns who the primary is and that aggregation happens there).

Roles are stored in **controller storage on the cluster level** (role inventory is in `07`; this feature owns that cluster-level controller storage exists and is the place for cluster-wide control data).

Every node can receive user requests (data path is not only the cluster primary).

## Why

Metadata and shared-type leadership are explicit, electable, and redundant. A restarted node reconstructs local datatype state without hand-built recovery.

## Actors

- Cluster primary/secondary (Raft) owning namespaces, nodes, drives
- Namespace primary/secondary owning schemas
- Datatype primary/secondary owning shared datatype metadata/data leadership
- Node restoring definitions always, durable content from WAL/disk, memory-mode empty unless replicated
- Client that may hit any **member** node for data, while controllers remain electable

## Requirements

- Four subordination levels as listed; datatype primary is metadata/leadership, not the write serializer.
- Primary + secondary at each controller level; secondaries used for failover and load balancing.
- RAFT elections at each controller level; minority partition does not mutate membership/schema.
- Restore: definitions always; persistent/hybrid content from drives; memory-mode content volatile (`13`).
- Consume membership from `11`; internode Raft RPCs on the `internode` handler (`12`).

## Out of scope for this feature

- RBAC role names (`07`); permission vocabulary (`14`)
- Metric names for elections (`08`) — but leader-election metrics MUST be emit-able from this control plane
- Graphical cluster map (`09`)
- Bootstrap/join/leave procedures (`11`) except storing the resulting membership
- WAL format (`13`) except invoking restore
- Clocks/LWW (`12`) except consuming HLC for metadata versions

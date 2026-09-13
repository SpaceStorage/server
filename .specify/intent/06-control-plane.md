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

- **Cluster-level primary** — contains all namespaces and nodes and drives.
- **Namespace-level primary** — contains all schemas and data inside them.
- **Datatype-level primary** — contains all data of shared datatypes.
- **Node-level** — stores state of all local datatypes in drives or memory.

All primaries MUST have their own **secondary servers** for failover and load balancing.

Different levels of controllers MUST implement **elections using RAFT**.

When a node comes up it MUST **restore state of all local datatypes** in drives or memory. All data MUST restore all data and their state.

Shared datatype metrics are aggregated by the **primary server** and stored in memory (series catalog is in `08`; this feature owns who the primary is and that aggregation happens there).

Roles are stored in **controller storage on the cluster level** (role inventory is in `07`; this feature owns that cluster-level controller storage exists and is the place for cluster-wide control data).

Every node can receive user requests (data path is not only the cluster primary).

## Why

Metadata and shared-type leadership are explicit, electable, and redundant. A restarted node reconstructs local datatype state without hand-built recovery.

## Actors

- Cluster primary/secondary (Raft) owning namespaces, nodes, drives
- Namespace primary/secondary owning schemas
- Datatype primary/secondary owning shared datatype metadata/data leadership
- Node restoring WAL/disk/memory datatype state at boot
- Client that may hit any node for data, while controllers remain electable

## Requirements

- Four subordination levels as listed.
- Primary + secondary at each controller level; secondaries used for failover and load balancing.
- RAFT elections at each controller level.
- Full local restore of datatypes and their state on node start.

## Out of scope for this feature

- RBAC role names (`07`)
- Metric names for elections (`08`) — but leader-election metrics MUST be emit-able from this control plane
- Graphical cluster map (`09`)

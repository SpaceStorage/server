# Contract: Cluster store (Raft-backed `ClusterStore`)

**Feature**: `006-control-plane` | Crate: `controlplane` | Spec: FR-003, FR-007, FR-012 | Seam: `004` `ClusterStore`

## Trait (unchanged signatures)

`append(event) -> Result<Applied>` commits via the **cluster** Raft group when the event is cluster-scoped, or the **namespace** group when it names a container/schema. Callers do not choose a group; the impl routes.

`snapshot() -> ClusterState` is the applied cluster machine plus pointers to namespace applied states.

Interim LWW `CatalogDelta` shipper is **removed** once this impl is wired (`004` R3).

## Cluster log MUST contain

- Membership view (`011`)
- Namespace **list** (not schemas)
- Nodes, drives, memory inventory pointers
- Role blob (opaque)
- Cluster `VoterSet`
- `exclusive_data` flag (slice 7)

MUST NOT contain schemas or container definitions (`FR-003`).

## Commit rule

A cluster metadata mutation (join recorded, create namespace, role blob replace) succeeds only with a **majority of cluster voters**. Minority → `Minority { group: cluster }` (`FR-007`). Non-voters reachable on the minority side do not count.

## Bootstrap

First node (`011` explicit bootstrap): cluster group voter set `{self}`, empty namespace list, empty roles. Creates no second cluster if another UUID exists on disk.

---
speckit_command: specify
suggested_slug: identity-membership
source: gap-analysis-2026-09-14
read_after: 00-constitution.md
---

# Feature: Cluster identity, discovery, join, leave, and replace

Specify how a SpaceStorage cluster is named, how nodes identify themselves, how they find each other, and how membership changes (bootstrap, join, drain, decommission, replace a dead node).

## What

Every cluster MUST have a **cluster name** (human label, not unique in the universe) and a **cluster identity**: a **random UUID** generated at bootstrap that does not change for the life of that cluster, plus a **join secret**. The name is not identity. Two bootstraps with the same name are **two clusters**. There is no protocol that detects or merges them across isolated networks. A node without the join secret MUST NOT be able to speak `internode` / `replication` (`12`).

Every node MUST have:

- a **node identity** (stable unique identifier, persisted on first start, independent of hostname and of listen addresses)
- a **node name** (human label, unique inside the cluster)

Nodes MUST NOT treat hostname, IP address, or process ID as identity. Addresses MAY change; identity MUST NOT.

### Discovery and bootstrap

A node finds the cluster through a documented **seed list** of already-member nodes (names or addresses). Exactly one of:

- **Bootstrap**: the first node, with an explicit bootstrap declaration and an empty or self-only seed list, creates the cluster identity (UUID + join secret) and becomes the initial cluster-level controller.
- **Join**: a subsequent node contacts a seed, presents the join secret, its identity and labels. Labels MUST include every key on the cluster **topology ladder** (`04`); omit or hierarchy-integrity failure is a refused join. The secret is **necessary** to open the cluster ports. **Membership still requires `CLUSTER_ADMIN` admit** or a **one-time join token** (bound to node name/id, single use, TTL on the order of hours, audit-logged) before the node serves tenant data as a replica target.

A node that is not bootstrapping and cannot reach any seed MUST refuse to become `ready` as a member.

The **join secret** MUST be rotatable with an overlap window (old and new both accepted, then old dies). Stolen secret without admit/token MUST NOT add a member.

### Join, drain, leave, replace

- **Join**: membership is recorded cluster-wide; every live member MUST converge on the same membership view. Joining does not by itself move existing replicas (rebalancing is `04`).
- **Drain**: an operator marks a node `draining`. The node MUST stop accepting new connections for tenant protocols, finish in-flight work up to the drain timeout (`01`), and MUST NOT be chosen for **new** replica placements. Existing replicas remain until rebalancing or decommission removes them.
- **Decommission (leave)**: replicas on the node MUST be re-placed onto other members that satisfy the containers' constraints (`04`). Only after those containers report their constraints satisfied (or the operator explicitly accepts data loss for unreplicated containers) MAY the node be removed from membership.
- **Replace dead node**: a new process MAY take over a dead member's **node identity** so that placements that named that identity do not all have to be rewritten. The replacement MUST prove authorization (cluster admin or the documented replace procedure). A live node MUST refuse to be replaced.

Rolling restart: drain one node at a time; restart; wait until `ready` and replica catch-up before draining the next. Mixed-version rules are in `15`.

### Relationship to the control plane

Cluster membership is **input** to Raft controller groups (`06`). A node that is not a member MUST NOT vote. Membership changes MUST be recorded in cluster-level controller storage so they survive restarts.

Every node that is a member MAY receive client requests (constitution: every node is a coordinator), including a node that holds no replica of the target container.

## Why

Without identity and membership, Raft cannot start, placement cannot name replica locations, and operators cannot add, remove, or replace machines without guessing.

## Actors

- Cluster architect bootstrapping the first node and publishing seeds
- Operator joining a node, draining it, decommissioning it, or replacing a dead one
- Cluster-level controller accepting or refusing join/leave/replace
- Rebalancer (`04`) consuming membership changes
- Client that may hit any **member** node

## Requirements

- Cluster **name** (label) + **UUID identity** + **join secret**; stable node identity + unique node name.
- Seed-based discovery; explicit bootstrap of the first node; join = secret + **admit or one-time token**.
- Join presents every key on the cluster topology ladder (`04`); hierarchy integrity or omit is refused.
- Same human name on two bootstraps is two clusters, not a merge event.
- Drain, decommission, and replace procedures as specified; replace reuses node identity of a dead member only.
- Membership view is identical on every live member; non-members do not vote and are not replica targets for new placements.
- Drain excludes the node from new placements and from new tenant connections.

## Out of scope for this feature

- Internode wire, clocks, conflict resolution (`12`)
- WAL/fsync durability (`13`)
- Raft election internals beyond consuming membership (`06`)
- Replica rebalancing algorithms (`04`) except as a consumer of drain/leave
- AuthN of the operator performing join/leave (`14`) except that these operations require the admin role
- Wire protocol handlers (`02`)

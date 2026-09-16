# Feature Specification: Control-Plane Hierarchy, Raft Elections, and Node Restore

**Feature Branch**: `006-control-plane`

**Created**: 2026-09-14

**Updated**: 2026-09-16

**Status**: Draft

**Input**: User description: "Read .specify/intent/06-control-plane.md and specify this feature." — four subordination levels (cluster, namespace, datatype, node) with Raft-elected primaries and secondaries; controllers own metadata and leadership requests, not per-write serialization; restore definitions always, durable content from drives, memory-mode volatile; membership from `11` recorded before a node votes; minority partitions must not mutate membership or schema.

## Clarifications

### Session 2026-09-16

- Q: When the cluster has many member nodes, which of those nodes cast votes in the cluster-level Raft election? → A: A dedicated odd-sized controller replica set votes; other members are non-voting. A controller voter MUST be easy to migrate to another member. By default those voters are equal members and MAY hold tenant/other data; a configurable option MAY prohibit housing non-controller data on them (off by default).
- Q: How should namespace-level and datatype-level primaries be elected relative to that cluster-level Raft group? → A: One Raft group per namespace (dedicated odd-sized, migratable voters; same default co-location of tenant data as cluster). Shared-datatype metadata lives in that namespace store. Single-writer / total-order leadership is a lease from that namespace group, not a Raft group per container.
- Q: Which parts of this control plane must already work in the first shippable binary (slices 1–5 in `16`), and which wait until slice 7? → A: First binary: cluster Raft plus a namespace Raft for every namespace that exists. Leadership leases only when an ordered type is created (first-binary types are leaderless). Voter migration and the exclusive-data option wait for slice 7.
- Q: When a log-stream’s leadership lease holder is cut off and the namespace grants a new lease, how must the old writer be stopped from appending? → A: Epoch/term fencing. Appends carry the lease epoch from the namespace Raft group; a stale epoch is refused.
- Q: Which controller should hold the in-memory aggregate of a shared datatype’s metrics? → A: The namespace primary for that datatype’s namespace. Not the cluster primary, not a leadership lease holder, not scraper-only merge.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Elect a cluster primary and keep a secondary (Priority: P1)

An operator bootstraps a cluster (`11`) and adds members. Cluster-level Raft votes are cast only by a **dedicated odd-sized controller replica set**, not by every member. That set elects a primary and at least one secondary. The primary's store lists namespaces, nodes, and drives. If the primary fails, a secondary becomes primary without operator reconstruction. A partitioned **minority of the voter set** cannot accept join/leave or schema-affecting cluster changes. Every **member** node still accepts client data requests (data path is not only the cluster primary), including non-voters.

**Why this priority**: Nothing else in the control plane exists without an electable cluster metadata owner.

**Independent Test**: Three-node cluster (all three voters): observe primary+secondary, stop the primary, confirm a new primary, confirm the minority of a 1+1 partition cannot join a fourth node. First binary (`16` slices 1–5) MUST pass this. Then (complete product / extra members) add two non-voting members, stop a non-voter, and confirm cluster metadata still progresses. Delivers electable cluster metadata without ordered-type leases.

**Acceptance Scenarios**:

1. **Given** three members that are the cluster voter set, **When** elections complete, **Then** exactly one cluster-level primary is reported and at least one secondary exists, and every member lists the same membership.
2. **Given** the cluster primary is stopped, **When** the remaining **voters** elect, **Then** a former secondary becomes primary and the membership view is unchanged.
3. **Given** a minority partition of the **voter set**, **When** an operator tries to join a node or create a namespace through that minority, **Then** the change is refused; the majority side remains the source of truth.
4. **Given** any member (voter or not), **When** a client issues a data query, **Then** that member may coordinate it even if it is not the cluster primary.
5. **Given** five members of which three are cluster voters, **When** a non-voter is stopped, **Then** cluster metadata writes still succeed; **When** two of three voters are partitioned away, **Then** metadata writes are refused.

---

### User Story 2 - Namespace Raft and datatype leadership leases (Priority: P1)

Each namespace has its **own Raft group** with a dedicated odd-sized voter set (migratable, same default co-location of tenant data as cluster). That group elects a namespace primary and secondary that own schemas, container **definitions**, and **shared-datatype metadata** for that namespace. Datatype-level “primary” is not a separate Raft group: types that need a single writer or total order obtain a **leadership lease** from the namespace group. An ordinary replicated key/value container does not take a lease and does not wait on that lease holder (`12`/`04`).

**Why this priority**: Intent's four levels are otherwise just names. Ties with Story 1.

**Independent Test**: First binary: create the namespace used by PostgreSQL/Redis (`16`), confirm its Raft group holds schemas and KV/table definitions, write to KV without a leadership lease. Complete product: two namespaces, plus a log-stream lease. Delivers namespace Raft without requiring ordered types in the first binary.

**Acceptance Scenarios**:

1. **Given** namespace `N`, **When** it is created, **Then** a distinct namespace Raft group exists with a primary and secondary, and it holds `N`'s schemas, container definitions, and shared-datatype metadata.
2. **Given** a replicated `K/V Store`, **When** clients write through any replica, **Then** writes do not wait for a datatype-level lease holder to serialize them (leaderless data path; no per-container Raft).
3. **Given** a `Log Stream` (or other type that requires total order), **When** it requests leadership, **Then** the namespace Raft grants a leadership lease **with an epoch** and appends are ordered by that lease holder. **Given** that holder is partitioned and a new lease is granted, **When** the old holder appends with the stale epoch, **Then** those appends are refused and MUST NOT appear in the ordered history.
4. **Given** a namespace primary failure, **When** election completes, **Then** definitions and shared-datatype metadata remain available via the new primary and no container definition is lost.
5. **Given** two namespaces, **When** one namespace's voter majority is lost, **Then** the other namespace still accepts schema and definition changes.

---

### User Story 3 - Restore on boot (Priority: P1)

A node restarts. Definitions and options of every local container return. Persistent and hybrid content returns from drives (WAL replay invoked from `13`). Memory-mode content returns empty unless replication re-populates it. The node does not vote or become a new replica target until membership (`11`) already records it. Voting still requires that member to be in the controller voter set (FR-010).

**Why this priority**: Constitution restore rule; previously contradicted memory-mode.

**Independent Test**: Create persistent, hybrid, and unreplicated memory-mode containers; restart the node; compare definitions vs content.

**Acceptance Scenarios**:

1. **Given** persistent and hybrid containers with data, **When** the node restarts, **Then** definitions, options, and content are present.
2. **Given** an unreplicated memory-mode container with data, **When** the node restarts, **Then** the definition and options are present, content is empty, and the description states the content did not survive.
3. **Given** a replicated memory-mode container, **When** the node restarts, **Then** replication may re-populate content (`04`); the node does not invent local content.
4. **Given** a process that is not yet in the membership view, **When** it starts, **Then** it does not vote and is not chosen for new replica placements.

---

### User Story 4 - Migrate a controller voter (Priority: P1; first required in slice 7)

An operator moves **cluster or namespace** controller voting off one member onto another without rebuilding that group's store. During the move the voter set stays odd-sized and keeps a majority. By default the old and new voters remain ordinary members: they MAY hold tenant (and other non-controller) data. A documented option MAY prohibit housing that other data on controller voters (cluster or namespace); it is **off by default**. Turning the option on MUST NOT leave tenant replicas stranded on those nodes (`04` rebalance / `11` drain consume the “not a tenant replica target” flag). The first shippable binary MAY run with a static voter set (the members that exist) and the option off; slice 7 MUST deliver migration and the exclusive-data option.

**Why this priority**: A dedicated voter set is unusable in production if replacing a controller node means reconstructing the cluster.

**Independent Test**: Five-member cluster with three voters A,B,C; migrate C → D; confirm D votes, C does not, metadata survives. Place a KV replica on a voter (default). Enable the exclusive-controller option and confirm new tenant replicas are not placed on voters.

**Acceptance Scenarios**:

1. **Given** cluster (or namespace) voters A,B,C and member D, **When** the operator migrates the C voter role to D, **Then** that group's store is unchanged, D is a voter, C is a non-voting member of that group, and a subsequent primary failure still elects from the new set.
2. **Given** a migration that would leave the voter set without a majority (or not odd-sized) at any step, **When** it is submitted, **Then** it is refused and the previous voter set remains.
3. **Given** the default (option off), **When** a tenant container is placed, **Then** a controller voter MAY be chosen as a replica target like any other member (`04`).
4. **Given** the option to prohibit non-controller data on controller voters is on, **When** new tenant replicas are placed, **Then** voters are excluded; existing tenant data on those nodes is drained/rebalanced (`04`/`11`) rather than left as a silent extra copy.

---

### User Story 5 - Secondaries for failover and read of metadata (Priority: P2)

Secondaries exist on the **cluster and namespace** Raft groups for failover and load balancing of **metadata reads** (describe cluster, list schemas). Metadata writes go to that group's primary. Shared-datatype metrics are aggregated **in memory on that namespace’s primary** (`08` names the series).

**Why this priority**: Intent requires secondaries for load balancing, not only failover.

**Independent Test**: Issue describe/list against a secondary while the primary is busy; stop primary; confirm failover.

**Acceptance Scenarios**:

1. **Given** a healthy primary and secondary, **When** an operator lists namespaces via the secondary, **Then** the list matches the primary.
2. **Given** a metadata write (create namespace), **When** it is submitted to a secondary, **Then** it is forwarded to the primary or refused with a redirect; it is not applied only on the secondary.
3. **Given** shared-datatype metrics, **When** they are requested, **Then** the aggregated in-memory view is served by that namespace’s primary, not by the cluster primary and not by a leadership lease holder.
4. **Given** that namespace primary is down, **When** local node metrics are scraped, **Then** they remain available and the shared-datatype aggregate is stale until a new namespace primary exists.

---

### Edge Cases

- Double bootstrap of two clusters with the same name: detected and not merged (`11`).
- All secondaries down: primary still serves; new secondaries can be added; durability of metadata follows Raft log + `13`.
- Corrupt local files during restore: isolate and rebuild or degrade (`13`); controller still comes up if metadata is intact.
- Mixed-version N/N+1: old node refuses new metadata formats (`15`).
- Leadership lease holder down for a leaderless KV container: data path continues (no lease). Metadata changes (RF, labels) wait for the **namespace** primary.
- Leadership lease holder down for an ordered type: appends wait until the namespace group grants a new lease **with a new epoch**; the old holder’s stale-epoch appends are refused (no split-brain history).
- Namespace primary down: local node metrics remain scrapeable; that namespace’s shared-datatype in-memory aggregate is stale until election completes (`08`).
- Single-member cluster: voter set size 1 (odd); adding the second and third voters is a migration/expand of that set.
- Controller exclusive-data option enabled while a voter still holds tenant replicas: refuse or block until those replicas are moved (`04`); do not drop them.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The control plane MUST implement four subordination levels: cluster, namespace, datatype, and node. Cluster and namespace elect via Raft. Datatype-level is shared-datatype metadata in the namespace store plus optional leadership leases from that namespace group (not a Raft group per container). Node-level is local restore, not an election.
- **FR-002**: Cluster-level and each namespace-level controller MUST run Raft elections and MUST have primary plus secondary for failover. Votes at each of those groups MUST be cast only by a dedicated odd-sized **controller replica set**, not by every member. Non-voting members MUST still coordinate client data (FR-008).
- **FR-003**: Cluster-level store MUST contain namespaces, nodes, drives, and the membership view supplied by `11`. It MUST NOT be the store for a namespace's schemas or container definitions.
- **FR-004**: Each namespace MUST have its own Raft group. That namespace-level store MUST contain schemas, container definitions, and shared-datatype metadata for that namespace.
- **FR-005**: Shared-datatype metadata MUST live in the namespace store. Datatype-level MUST NOT run a separate Raft group per container. It MUST NOT serialize ordinary leaderless writes (`12`).
- **FR-006**: Types that require a single writer or total order MUST obtain a **leadership lease** from their namespace Raft group. The lease MUST carry a monotonically increasing **epoch**. Every append (or other totally ordered write) MUST present that epoch; a stale epoch MUST be refused and MUST NOT enter the ordered history. Leaderless types MUST NOT take that lease. Wall-clock timeout alone MUST NOT be the fence.
- **FR-007**: A partitioned minority of a group's **voter set** MUST NOT accept that group's mutations: cluster minority MUST NOT accept membership (or other cluster-store) changes; namespace minority MUST NOT accept schema, definition, or shared-datatype metadata changes, or grant leadership leases. Non-voters do not count toward that majority.
- **FR-008**: Every member node MUST be allowed to coordinate client data requests regardless of who is cluster primary.
- **FR-009**: On start, restore definitions and options of all local datatypes; restore persistent/hybrid content via `13`; restore memory-mode content as empty unless replication re-populates.
- **FR-010**: A node MUST NOT vote or be a new replica target until cluster-level storage records it as a member (`11`). Membership is necessary to vote; it is not sufficient — only nodes in the dedicated voter set vote.
- **FR-011**: Secondaries MUST serve metadata reads for load balancing; metadata writes MUST be applied by the primary (forward or redirect).
- **FR-012**: Roles MUST be stored in cluster-level controller storage (`07`/`14`).
- **FR-013**: Shared-datatype metrics MUST be aggregated in memory by the **namespace primary** of the namespace that owns that datatype, for `08`. The cluster primary MUST NOT be the aggregator. A leadership lease holder MUST NOT be the aggregator. Each node still emits metrics only for its own local datatypes (`08`).
- **FR-014**: Raft RPCs MUST use the `internode` handler (`12`).
- **FR-015**: Metadata versions MUST be stamped with HLC (`12`) where a version is required.
- **FR-016**: Leader-election totals, errors, and duration MUST be emit-able for `08` (names live in `08`).
- **FR-017**: Controller secondaries MUST be usable to replace a failed primary without operator-built reconstruction of the store.
- **FR-018**: An operator MUST be able to migrate a cluster- or namespace-controller voter to another member without reconstructing that group's store. The procedure MUST keep an odd-sized voter set and a majority at every accepted step; a step that would not MUST be refused. This MUST be present from slice 7; the first binary MAY use a static voter set.
- **FR-019**: By default, cluster- and namespace-controller voters MUST be equal members and MAY hold tenant and other non-controller data. A documented configuration option MUST exist to prohibit housing that other data on those voters; the option MUST be off by default. When it is on, `04` MUST treat those nodes as excluded tenant replica targets; existing tenant replicas MUST be drained/rebalanced, not dropped. The option MUST be present from slice 7; the first binary MUST behave as option-off.
- **FR-020**: The first shippable binary (`16` slices 1–5) MUST run cluster-level Raft and a namespace Raft group for every namespace that exists. It MUST NOT require leadership leases unless an ordered type is created (first-binary types in `16` are leaderless). Voter migration (FR-018) and the exclusive-data option (FR-019) wait for slice 7. This tightens `16`'s “control plane Raft in slice 7” to: Raft for cluster and existing namespaces is in the first binary; remaining `06` surface is slice 7.

### Key Entities

- **Cluster Controller**: Raft group owning membership, namespaces list, nodes, drives, role store. Voters are a dedicated odd-sized replica set, not all members.
- **Controller Voter Set**: The members that vote in one Raft group (cluster or a namespace). Migratable; odd-sized; majority is computed only over this set.
- **Namespace Controller**: One Raft group per namespace, owning schemas, container definitions, and shared-datatype metadata for that namespace. Same voter-set and data-housing rules as cluster. The namespace **primary** holds the in-memory aggregate of that namespace’s shared-datatype metrics.
- **Datatype Level**: Not a Raft group. Shared-datatype metadata in the namespace store; optional **leadership lease** from that namespace group for single-writer / total-order types.
- **Node Local State**: Definitions, options, and durable content on one member; memory-mode content volatile.
- **Primary / Secondary**: Elected roles in a cluster or namespace Raft group (among that group's voters).
- **Leadership Lease**: Granted by a namespace Raft group to a type that requested a single writer. Carries a monotonically increasing **epoch**. Not used for leaderless containers. A holder with a stale epoch MUST NOT append.
- **Controller Data-Housing Option**: Off by default. When on, cluster and namespace controller voters MUST NOT house non-controller (tenant) data.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: In 100% of three-node conformance runs, exactly one cluster primary exists and at least one secondary exists after election.
- **SC-002**: After killing the cluster primary, a new primary is elected and membership is unchanged in 100% of runs.
- **SC-003**: 100% of membership or other cluster-store change attempts on a minority of the **cluster voter set** are refused, including when extra non-voting members are reachable on the minority side. 100% of schema, definition, or lease-grant attempts on a minority of a **namespace voter set** are refused.
- **SC-004**: After restart, 100% of persistent/hybrid containers in the suite keep definition and content; 100% of unreplicated memory-mode containers keep definition, empty content, and a volatility notice.
- **SC-005**: 100% of leaderless KV writes in the suite complete without requiring the datatype primary to be reachable as a write serializer.
- **SC-006**: When ordered types are in the suite (complete product; not required for first-binary types in `16`), 100% of those appends are totally ordered by the namespace-granted leadership lease holder. 100% of leaderless types in the suite have no such lease. After a lease re-grant, 100% of stale-epoch appends from the old holder are refused and 0 of them appear in the ordered history.
- **SC-007**: 100% of metadata-read samples against a secondary match the primary.
- **SC-008**: Leader-election counts and duration are available to observability in 100% of election tests.
- **SC-009**: From slice 7, in 100% of voter-migration runs, the store is unchanged, the destination member votes, the source does not, and a later primary failure still elects. Not required for the first binary.
- **SC-010**: From slice 7, with the exclusive-data option off, 100% of the suite's placements MAY put tenant replicas on a controller voter. With it on, 100% of new tenant replica placements exclude those voters, and 0 tenant replicas are dropped in place. First binary is option-off only.
- **SC-011**: 100% of shared-datatype aggregate samples in the suite are served from that namespace’s primary. 0 are served from the cluster primary or from a leadership lease holder as the aggregator.

## Assumptions

- Bootstrap, join, drain, decommission, replace **node identity** procedures are `11`; this feature stores the resulting membership, owns the **controller voter set** (who votes), and refuses votes from non-members and from members not in that set.
- Typical voter-set sizes are 1 (single node), 3, or 5; this spec requires odd size, not a specific N.
- Leaderless data path and HLC/LWW are `12`/`04`; this feature supplies a namespace-granted leadership lease only when requested, fenced by lease epoch. There is no datatype-level Raft group. LWW MUST NOT be used to repair two ordered histories.
- WAL replay is `13`; this feature invokes restore and applies the definition/content split.
- Role names and permission checks are `07`/`14`; this feature is the store.
- Metric names are `08`. This feature names the aggregator: the namespace primary.
- "Load balancing" on secondaries means metadata reads, not tenant query coordination (any member already coordinates queries).
- First binary vs slice 7 is FR-020. `16` still sequences tenancy quotas and full authz in slice 7; this feature only pulls cluster and existing-namespace Raft into slices 1–5.

## Out of Scope

- Join/leave/replace **procedures** (`11`) except persisting membership.
- Internode framing and clocks (`12`) except using `internode` and HLC stamps.
- WAL format (`13`) except invoking restore.
- RBAC vocabulary (`14`) and role **names** inventory (`07`) except storing them.
- Graphical cluster map (`09`).
- Quorum arithmetic for user data (`04`), except consuming the “controller voter is / is not a tenant replica target” flag when the exclusive-data option is set.

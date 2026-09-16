# Feature Specification: Distribution, Placement, Media, and Replication (L1 Shared Capabilities)

**Feature Branch**: `004-distribution-placement`

**Created**: 2026-09-14

**Status**: Draft

**Input**: User description: "Read .specify/intent/04-distribution-placement.md and specify this feature." — the intent file defines how the shared (L1) capabilities place and copy data across nodes: replication, sharding, partitioning, node placement, failure handling, rebalancing, consistency, persistent placement, labels / placement constraints, distributed transactions, consensus, leader election, and quorum. Nodes carry labels (`rack`, `az`, `region`, and further) so replicas can be spread with anti-affinity by a selected label; drives (SSD, HDD, NVMe, and others) enrich node labels so different data can select different media; in-memory storage is declared explicitly per node with a size and labels and may be absent on some nodes. Storage and drivers specify a Cassandra-style quorum level per query, protocols without such a field supply it through options or fall back to the global defaults (write waits for two acknowledgements, read is satisfied by one). Every node can receive requests from users and operate with data. Replication may be synchronous — which must implement quorum for writes — or asynchronous — which must implement quorum for reads — and all replication logic is based on datatypes and data composition. All options must be documented with starter examples while staying flexible enough to build clusters across regions, continents, or planets, keeping data near the user and replicating slowly to remote regions.

## Clarifications

### Session 2026-09-14

- Q: When a client writes to a replicated container, must any particular replica serialize that write, or can every replica accept it independently and the coordinator only wait for enough acknowledgements? → A: Leaderless: any replica accepts the write; the coordinator waits for the quorum acknowledgements. A datatype-level primary owns metadata only, not per-write serialization. Types that need a single writer still request leadership from the control plane.
- Q: When two replicas of the same key hold different values, which value should the cluster keep? → A: Last-writer-wins by a per-write version stamp. Clock skew is measured and reported. A container may declare a type-supported alternative (for example an order-insensitive merge).
- Q: When a write waits for acknowledgements, may a replica that stores the data only in memory count toward the same quorum as a replica that stores it on a drive? → A: Persistent and hybrid containers count only drive-backed acknowledgements toward write quorum. Memory-mode containers count memory acknowledgements. Mixed replica sets report which acknowledgements were durable.
- Q: If a container replicates locally in lockstep but sends a slower copy to another region, what should happen when a client asks for an acknowledgement from every region? → A: Refuse `EACH_QUORUM` (and any level that requires a remote acknowledgement) on a container with asynchronous remote replication, unless the container declares that such requests wait. Default is refuse.
- Q: If a designer asks for three copies that must sit in different availability zones, but the cluster only has two zones, should the cluster still place the container? → A: Refuse when the topology cannot spread replicas across the requested label. No silent downgrade. A separate, explicit best-effort mode may exist later; it is not the default.

### Session 2026-09-15

- Q: Are `rack` / `az` / `region` / `continent` / `planet` a product-wide fill-in, or a cluster-declared ladder? → A: Cluster-declared **topology ladder**: an ordered subsequence of the reserved near→far keys `rack`, `az`, `region`, `continent`, `planet`. Unused rungs may be omitted. Custom keys MAY append after reserved names the cluster uses. Every member node MUST supply every key on **that** ladder; omit or hierarchy-integrity failure refuses join (`11`) or prevents `ready` as a replica target. A laptop is not forced to invent `planet` unless the ladder includes it. **`planet` is a label, never an HLC or write-quorum domain.**
- Q: What is the voting / HLC set if not `planet` or `region`? → A: An explicit **`quorum_domain`** (`12`). The ladder is farness for placement and planning, not a voting set. Write `ONE` / `LOCAL_ONE` / `TWO` / `QUORUM` count only durable replicas in the container's **source** domain. Log-followers never count toward those write levels. `LOCAL_*` for **reads** is the coordinator's domain. Writes that land on a follower **forward to the source**.
- Q: How should the planner rank replicas that already satisfy quorum? → A: Rank by the first ladder key that differs (near before far). **Measured internode RTT** (and HLC skew as a health signal, `12`) MUST override that rank once samples exist. A missing ladder value is not “local”.
- Q: What is default anti-affinity for RF≥2? → A: The **finest** key on the cluster ladder (usually `az` or `rack`). The operator MAY name a coarser key. Unsatisfiable spread is refused.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Describe the cluster: nodes, labels, drives, and memory (Priority: P1)

A cluster architect describes what the cluster physically is. The cluster declares an ordered **topology ladder** — a subsequence of the reserved farness keys `rack`, `az`, `region`, `continent`, `planet` (starter: `[az]` for a first binary / one room; `[rack, az, region]` for a production region). Each member node MUST fill every key on **that** ladder. Custom keys (`provider`, `jurisdiction`, …) MAY exist but are not farness unless placed on the ladder. Drive media and optional memory pools enrich labels as before. **`planet` is never a voting or HLC domain** — that is an explicit `quorum_domain` (`12`). The architect then asks the cluster what it sees: ladder, nodes, media, and how many distinct values each ladder key has — which is how many replicas can be spread apart by that key.

**Why this priority**: Nothing else in this feature exists without topology. Replication factor, anti-affinity, media selection, locality, and rebalancing are all expressed over labels, so the label and capacity model must come first.

**Independent Test**: Can be fully tested with a small cluster (and on a single node) by declaring labels, drives, and a memory pool on each node, reading the cluster topology view, changing a label, adding a drive, removing a memory pool, and submitting invalid declarations. Delivers a queryable, validated topology with no data placed yet.

**Acceptance Scenarios**:

1. **Given** a cluster whose topology ladder is `[rack, az, region]` and a node declaring labels `rack=rack1`, `az=az1`, `region=region1`, **When** the node joins the cluster, **Then** the cluster topology view reports the node with exactly those labels plus the ladder, and each label is visible to every other node.
1a. **Given** a cluster ladder `[az, region]` and a node that omits `region`, **When** it attempts to join, **Then** join is refused (`11`) or the node MUST NOT become `ready` as a replica target.
1b. **Given** two nodes that share `az=a1` but declare different `region` values, **When** the second is submitted, **Then** that is a hierarchy-integrity error and the join or label change is refused.
2. **Given** two nodes labelled `rack1/az1/region1` and `rack2/az2/region1` on ladder `[rack, az, region]`, **When** the placement domains are requested, **Then** the cluster reports 2 distinct values for `rack`, 2 for `az`, and 1 for `region`, default anti-affinity for RF≥2 is `rack` (finest ladder key), and states that these two nodes support anti-affinity by `rack` or `az` up to replication factor 2 and by `region` up to replication factor 1.
3. **Given** a node with one NVMe drive and two HDD drives declared, **When** the node's topology entry is described, **Then** each drive appears with its media kind, its capacity, its free space, and its own labels, and the node reports derived media labels covering `nvme` and `hdd` so that a constraint naming either matches this node.
4. **Given** a node declaring in-memory storage of a stated size with labels, **When** the topology is described, **Then** the memory pool appears with its size, its labels, and its current usage; **Given** a node that declares no in-memory storage, **When** the topology is described, **Then** the node reports no memory pool and is excluded from any placement that requires one.
5. **Given** a node declaration with a duplicate label key, a drive with no media kind, a memory pool with no size, or a memory pool larger than the machine allows, **When** it is submitted, **Then** the node refuses to start or the change is refused with an error naming the offending declaration; a partially valid declaration is never half-applied.
6. **Given** a running cluster, **When** an architect adds, changes, or removes a node label or adds a drive, **Then** the change is accepted while the node keeps serving traffic, the topology view reflects it, and every placement whose constraints the change affects is re-evaluated and reported as satisfied, degraded, or unplaceable.
7. **Given** the starter documentation, **When** an architect follows the single-node, single-rack, three-AZ, and two-region examples verbatim, **Then** each produces a cluster whose topology view matches the example's stated placement domains.

---

### User Story 2 - Replicate a container with a factor and anti-affinity by label (Priority: P1)

A schema designer declares that a container is replicated N times and that no two replicas may share a value of a chosen label — `az` for inter-AZ safety, `rack` for a single-room cluster, `region` for continental safety. The placement machinery picks the nodes, reports which ones it picked and why, and keeps the guarantee for the life of the container. When the topology cannot satisfy the request — three replicas with anti-affinity by `az` in a two-AZ cluster — the answer is an immediate, explicit refusal that names what is missing, not a silent placement that quietly violates the guarantee.

**Why this priority**: Replication with anti-affinity is the core survivability promise of the intent and the reason labels exist. It is the first capability declaration (from the type system feature) that this feature has to actually execute.

**Independent Test**: Can be fully tested on a small cluster by creating containers with factors 1, 2, and 3 under anti-affinity by each label key, describing the resulting placement, requesting an unsatisfiable combination, and restarting a node. Delivers surviving, inspectable replica sets without any of the later capabilities.

**Acceptance Scenarios**:

1. **Given** a cluster of six nodes across three AZs, **When** a container declares replication factor 3 with anti-affinity by `az`, **Then** three replicas are placed on nodes in three different AZs, the container's placement description names the chosen nodes, their labels, and the satisfied constraint, and the placement is recorded in cluster metadata so every node resolves it identically.
2. **Given** the two-node topology of Story 1, scenario 2, **When** a container declares replication factor 2 with anti-affinity by `az`, **Then** it is placed; **When** it declares replication factor 2 with anti-affinity by `region`, **Then** the declaration is refused before any storage is allocated with an error naming the label key, the required number of distinct values, the number available, and the nodes considered, and no replica is placed in a weaker spread.
3. **Given** a container declaring anti-affinity by an ordered list of keys (`region`, then `az`, then `rack`), **When** it is placed, **Then** replicas are spread across as many distinct `region` values as the factor allows, then across `az` within a region, then across `rack`, and the description states which level of the hierarchy each replica satisfies.
4. **Given** a placement that was satisfied at creation, **When** a node label change or a node removal would break the anti-affinity guarantee, **Then** the container is reported as degraded with the specific violated constraint, a repair placement is proposed or executed by the rebalancing capability, and the container keeps serving in the meantime.
5. **Given** a container with replication factor 3, **When** it is described, **Then** the description reports, per replica: the node, the node's labels, the drive or memory pool holding it, whether the replica is in sync, its replication lag if asynchronous, and its health.
6. **Given** a container declared with no replication capability at all, **When** it is created, **Then** it is placed on one node according to the cluster's documented default placement, the description states that it has a single copy and no anti-affinity guarantee, and the cluster defaults it reports match what the placement layer actually applies.
7. **Given** a replicated container, **When** the replication factor is raised, **Then** new replicas are placed under the same anti-affinity constraint and populated in the background while the container serves traffic; **When** the factor is lowered, **Then** surplus replicas are removed only after the remaining set satisfies the constraint.
8. **Given** any node in the cluster, **When** it is asked where a container's replicas are, **Then** it returns the same answer as every other node.

---

### User Story 3 - Pick a quorum per query, from any node (Priority: P1)

A client connects to whichever node is nearest and issues a write and a read. It may state a quorum level for that query in the Cassandra vocabulary. If its protocol has no field for that, it passes the level through the protocol's option mechanism, or it says nothing at all and the cluster's documented defaults apply: a write waits for two acknowledgements, a read is satisfied by one. The node that received the request coordinates it — fanning out to the replicas, counting acknowledgements, and answering — whether or not that node holds a replica itself. Whatever level was actually applied, and where it came from, is inspectable.

**Why this priority**: Quorum is the constitution's cross-protocol consistency contract and the protocol drivers feature already depends on this vocabulary and these defaults. Any-node coordination is the constitutional promise that there is no mandatory proxy.

**Independent Test**: Can be fully tested on a three-node cluster by issuing writes and reads at each quorum level from a node that holds a replica and from one that does not, omitting the level to observe defaults, stopping replicas to make a level unachievable, and inspecting the applied options. Delivers the full consistency contract independent of media, sharding, and multi-region topology.

**Acceptance Scenarios**:

1. **Given** a container with three replicas, **When** a client writes with no quorum stated, **Then** the write is acknowledged once two *counted* replicas have acknowledged it (drive-backed for a persistent container), and the applied level is reported as `TWO` with source `global` default.
2. **Given** the same container, **When** a client reads with no quorum stated, **Then** the read is answered from one replica, and the applied level is reported as `ONE` with source `global` default.
3. **Given** the same container, **When** a client states `QUORUM` on a write and `QUORUM` on a read, **Then** the write is acknowledged by a strict majority of replicas, the read consults a strict majority, and a value written under `QUORUM` is returned by the immediately following `QUORUM` read.
4. **Given** a client connected to a node holding no replica of the target container, **When** it writes and reads, **Then** the request is coordinated by that node, the same quorum rules apply, the result is identical to one issued at a replica node, and the coordinating node is named in the execution record.
5. **Given** a request whose stated quorum cannot be satisfied by the container's replication factor (`THREE` on a container with two replicas), **When** the level was stated explicitly by the client, **Then** the request is refused before execution naming the requested and available counts; **When** the level came from a default, **Then** it is clamped to the highest satisfiable level and the applied value reports what it was clamped from.
6. **Given** a container with three replicas of which two are unreachable, **When** a client writes at `QUORUM`, **Then** the write fails with an error naming the level required, the acknowledgements achieved, and the unavailable replicas, and the client is not told the write succeeded; **When** the client writes at `ONE`, **Then** the write succeeds on the reachable replica and the description of the container reports the others as behind.
7. **Given** a per-container consistency declaration, **When** a query states nothing, **Then** the container's declaration takes precedence over the global default, and the applied options report source `container`; **When** the query states a level, **Then** the query wins.
8. **Given** any executed request, **When** its applied options are inspected, **Then** they report the quorum level, its source, any clamping, the coordinating node, the replicas contacted, and the acknowledgements received.
9. **Given** a replicated container with no single-writer declaration, **When** a write is issued while the replica that last coordinated a write for that key is unreachable, **Then** another replica still accepts the write, the coordinator waits only for the stated acknowledgements, and the write is not refused for lack of a write leader.
10. **Given** a persistent container whose replica set includes a memory-only copy, **When** a client writes at `TWO` and only the memory copy plus one drive-backed replica acknowledge, **Then** the write waits until a second drive-backed acknowledgement arrives; the memory acknowledgement is reported but does not count. **Given** a memory-mode container with three memory replicas, **When** a client writes at `TWO`, **Then** two memory acknowledgements satisfy the write.

---

### User Story 4 - Put each datatype on the right media (Priority: P2)

A tenant has embeddings that must be searched from NVMe, an archive that belongs on HDD, and a hot counter that should live entirely in memory. Each container declares the media it needs as a constraint over drive and memory labels. Placement honours the constraint, refuses when no node offers that media, and reports exactly which drive or memory pool holds each replica. A container asking for memory is only placed on nodes that declared a memory pool with room for it.

**Why this priority**: Media selection is the second half of the intent's label model and the reason drives enrich node labels. It builds directly on Stories 1 and 2 and is what makes one cluster serve workloads that would otherwise need separate machines.

**Independent Test**: Can be fully tested on a cluster with mixed media by placing containers constrained to each media kind, placing a memory container on a cluster where only some nodes declare memory, filling a drive and a memory pool, and requesting absent media. Delivers per-datatype media control without multi-region concerns.

**Acceptance Scenarios**:

1. **Given** a cluster where two nodes have NVMe and four have HDD only, **When** a container declares a constraint requiring NVMe with replication factor 2 and anti-affinity by `rack`, **Then** it is placed on the two NVMe nodes if their racks differ, and the description names the specific drive holding each replica.
2. **Given** the same cluster, **When** a container declares a constraint requiring NVMe with replication factor 3, **Then** it is refused naming the media, the required and available node counts, and the nodes that were excluded and why.
3. **Given** a cluster where only some nodes declare in-memory storage, **When** a memory-mode container is created, **Then** it is placed only on nodes with a memory pool that has room for it, and nodes without a pool are reported as excluded for that reason.
4. **Given** a memory-mode container whose replicas are placed, **When** one of those nodes restarts, **Then** the local copy comes back empty as the type system defines, the replica is marked as repopulating, content is restored from a live replica, and the container's description distinguishes "repopulating from replica" from "content lost" — the latter only when no live replica remains.
5. **Given** a container with a media constraint, **When** every matching drive on a candidate node is full, **Then** that node is excluded from placement with "no capacity on matching media" as the stated reason, and the container is reported unplaceable rather than silently placed on other media.
6. **Given** a container placed on a given media, **When** the constraint is later changed to another media, **Then** replicas are moved by the rebalancing capability in the background without downtime, and the description shows both the current and the target media until the move completes.
7. **Given** a container that declares both a media constraint and a locality constraint (`region=eu` and NVMe), **When** it is placed, **Then** both constraints hold for every replica, and a topology satisfying only one of them yields a refusal naming the unsatisfied constraint.
8. **Given** a drive or a memory pool, **When** its usage is inspected, **Then** it reports capacity, used space, and which containers' replicas it holds.

---

### User Story 5 - Keep serving through a node, rack, or AZ failure (Priority: P2)

A rack loses power. Clients connected to surviving nodes keep reading and writing at the levels their quorum settings allow, and clients whose entry node died reconnect elsewhere and continue. The cluster marks the affected replicas as unavailable, keeps the writes those replicas missed, and when the rack returns it brings them up to date without an operator reconstructing anything. Reads never silently return data that the requested quorum did not justify.

**Why this priority**: Survivability is the payoff of Stories 1 to 3; without failure handling, anti-affinity is decoration. It is P2 only because it can be specified and verified after the placement and quorum contracts exist.

**Independent Test**: Can be fully tested by stopping one replica, then a majority, then all of a rack; writing and reading at each quorum level during the outage; restarting the nodes; and verifying convergence and the reported state at each step. Delivers verified fault tolerance on one cluster, one region.

**Acceptance Scenarios**:

1. **Given** a container with three replicas across three AZs, **When** one AZ becomes unreachable, **Then** writes and reads at `ONE`, `TWO`, and `QUORUM` continue to succeed, writes and reads at `ALL` fail naming the unavailable replicas, and the container is described as degraded with the reason and the affected replicas.
2. **Given** the failure above, **When** the missing replica returns, **Then** the writes it missed are applied to it without operator action, its description moves from "behind" to "in sync" with the outstanding amount visible while it catches up, and no acknowledged write is lost.
3. **Given** a replica that stays down longer than the cluster's documented retention window for missed writes, **When** it returns, **Then** it is brought up to date by a full comparison against a healthy replica rather than by replaying missed writes, the difference is reported, and it is not returned to service as in-sync until it is.
4. **Given** a read at a level that contacts several replicas whose answers disagree, **When** the read completes, **Then** the client receives the value with the later version stamp (the default last-writer-wins rule), the disagreement is recorded, and the stale replicas are corrected in the background; **Given** a container that declared a type-supported alternative merge, **When** the same disagreement occurs, **Then** that merge is applied instead and still recorded.
5. **Given** a coordinating node that dies mid-request, **When** the client reconnects to another node and retries, **Then** any node can serve the retry, and the container's documented write semantics for retries (what is idempotent and what is not) are stated in its description.
6. **Given** a network partition that splits the cluster, **When** clients write on both sides, **Then** only a side that can reach the required number of acknowledgements accepts the write at that level, a side that cannot refuses rather than accepting locally, and on heal the cluster keeps the later version stamp of each key (or the container's declared alternative), reports every resolved conflict, and leaves no replica holding the discarded value.
7. **Given** a whole node lost permanently and removed from the cluster, **When** it is decommissioned, **Then** every container that had a replica on it is re-placed on a node satisfying its constraints, the work is visible as progress, and the container's replica count returns to its declared factor.
8. **Given** any degraded container, **When** the cluster health is inspected, **Then** every degraded placement is listed with the container, the constraint at risk, the failure that caused it, and the repair in progress.

---

### User Story 6 - Shard and partition a container that outgrows one node (Priority: P2)

A container's data set is larger than any single node or its write rate exceeds one replica set. The designer declares a sharding key so the data is divided across replica sets, or a partitioning scheme — by range or by time — so it is divided into independently placed pieces, or both. Every node still accepts every request: the receiving node resolves the key to the right shard and partition, and a query that spans several of them is fanned out and combined. Replication, anti-affinity, media, and quorum apply per shard, so each piece keeps every guarantee the whole container had.

**Why this priority**: Sharding and partitioning are how the model scales past one machine and how the L4 distribution compositions of the type system are actually executed. They follow the single-replica-set guarantees rather than preceding them.

**Independent Test**: Can be fully tested by creating a sharded container and a partitioned container, writing keys that land in different shards, reading single-key and multi-key queries from a node holding none of the data, adding a shard, and submitting an invalid key declaration. Delivers horizontal scale with the Story 2 and 3 guarantees intact per shard.

**Acceptance Scenarios**:

1. **Given** a container declaring a sharding key and a shard count, **When** it is created, **Then** each shard is placed as its own replica set under the container's replication factor, anti-affinity, and media constraints, and the description lists every shard with its key range or hash range and its replicas.
2. **Given** a sharded container, **When** a client writes a key from any node, **Then** the receiving node routes the write to the shard the key belongs to, applies the write quorum within that shard's replica set, and acknowledges; the answer does not depend on which node received the request.
3. **Given** a query that spans several shards, **When** it is issued, **Then** the coordinating node contacts the shards involved, applies the quorum per shard, combines the results, and reports per-shard failure explicitly rather than returning a silently partial answer.
4. **Given** a container declaring a time-based partitioning scheme, **When** data spanning several periods is written, **Then** each period lands in its own partition, each partition is placed independently under the container's constraints, and a read bounded to one period contacts only that partition's replicas.
5. **Given** a partitioned container where different partitions declare different media or locality constraints (recent partitions on NVMe, older on HDD), **When** the container is described, **Then** the per-partition placement is reported, and reads work identically across partitions regardless of the media behind them.
6. **Given** a sharded container, **When** shards are added or the shard count is changed, **Then** the affected key ranges are moved in the background with the container still serving, every key resolves to exactly one shard throughout the move, and no acknowledged write is lost or duplicated.
7. **Given** a sharding key declaration that the container's type does not support, a partitioning scheme incompatible with the declared sharding key, or a key that does not exist in the container's schema, **When** it is submitted, **Then** it is refused naming the conflict, and nothing is placed.
8. **Given** a sharded container with a skewed key distribution, **When** the cluster is inspected, **Then** the per-shard data volume and request rate are visible, and a shard that exceeds the documented imbalance threshold is reported as a rebalancing candidate.

---

### User Story 7 - Keep data near the user and replicate slowly to remote regions (Priority: P3)

A service has users in two `quorum_domain`s. The architect places working data in the source domain and declares asynchronous log-followers in the other. Local clients write at levels that count only durable replicas in the source; they never wait on a follower round trip unless they opted into `EACH_QUORUM`. A write received in the follower domain is **forwarded to the source**. Readers in the follower domain may use `LOCAL_ONE` (stale local apply) or stronger levels that forward to the source. The topology **ladder** ranks candidate replicas (near before far); measured RTT overrides that rank. `planet` may appear on the ladder as a label; it is never the voting set.

**Why this priority**: Planetary reach is the intent's headline ambition and the constitution's Principle VII, but it composes the earlier capabilities rather than adding a new mechanism. It follows them and is validated with a latency-shaped topology.

**Independent Test**: Can be fully tested with a two-region topology (simulated latency is sufficient) by declaring local-synchronous and remote-asynchronous replication, measuring that local writes do not wait for the remote acknowledgement, reading locally and remotely, inspecting lag, cutting the link between regions, and healing it. Delivers geographic distribution without new placement primitives.

**Acceptance Scenarios**:

1. **Given** two `quorum_domain`s (`eu` source, `us` log-follower) and a ladder that includes `region`, **When** a container declares three replicas in `eu` replicated synchronously and two in `us` as async log-followers, **Then** the placement honours the per-domain counts and modes, and the description reports each replica's domain and that `us` does not count toward write `TWO`/`QUORUM`.
2. **Given** that container, **When** a client whose coordinator is in `eu` writes at a source-domain write level (`12`), **Then** the write is acknowledged once enough **durable** replicas **in `eu`** have acknowledged it, the elapsed time does not include a round trip to `us`, and `us` receives the write afterwards via the source log.
3. **Given** the same container, **When** a client whose coordinator is in `us` reads at `LOCAL_ONE`, **Then** it MAY be answered from a local applied replica and MAY be stale; **When** it reads at a stronger level, **Then** the read is forwarded to `eu` if `us` cannot satisfy it. A write received in `us` is forwarded to `eu`; `LOCAL_ONE` **write** still requires one durable WAL in `eu`.
4. **Given** an asynchronous replication stream, **When** it is inspected, **Then** it reports the outstanding backlog, the current lag in time and in pending writes, and the configured lag threshold above which it is reported as unhealthy.
5. **Given** the link between the two domains is cut, **When** clients keep writing in `eu` at a source-domain write level, **Then** those writes succeed, the backlog to `us` grows and is reported, and `us` readers are told how stale their applied replica is; **When** the link is restored, **Then** the backlog drains by applying the source log, and convergence is reported without operator action.
6. **Given** `EACH_QUORUM` stated on a container whose remote replication is asynchronous and that has not opted into waiting, **When** the request is issued, **Then** it is refused before waiting on the follower domain; **Given** the same container with wait-for-apply declared, **When** `EACH_QUORUM` is stated, **Then** the write waits for required quorum in the source **plus** apply of that log position in the follower (`12`).
7. **Given** a topology whose inter-domain round trip is extremely long (minutes; a `planet` label MAY be on the ladder), **When** containers are placed and replicated, **Then** no timeout, lag threshold, or retry budget is fixed in a way that assumes a low round trip. HLC is **not** compared across `quorum_domain`s. The documentation includes an example with a round trip measured in minutes.
8. **Given** the starter documentation, **When** an architect follows the two-domain example verbatim, **Then** the resulting cluster matches the described placement, modes, and quorum behaviour.

---

### User Story 8 - Rebalance after the cluster changes (Priority: P3)

An operator adds three nodes to a full cluster, retires an old rack, or relabels a node that was moved to another AZ. The cluster computes what has to move to satisfy every container's declared constraints again, shows the operator the plan before or while it runs, moves the data in the background at a rate the operator controls, and never drops below the availability the containers' quorum settings promise. The operation can be paused, resumed, and inspected, and it converges to a state where every container reports its constraints satisfied.

**Why this priority**: Rebalancing is what keeps the earlier guarantees true over the cluster's life, but it can only be specified once placement, replication, media, failure handling, and sharding exist.

**Independent Test**: Can be fully tested by adding a node to a loaded cluster, decommissioning a node, changing a node's label, and changing a container's replication factor and constraints; observing the plan, the progress, and the throttle; pausing and resuming; and verifying constraint satisfaction and data integrity at the end. Delivers safe topology evolution.

**Acceptance Scenarios**:

1. **Given** a balanced cluster and three new nodes, **When** they join, **Then** the cluster produces a rebalancing plan naming every replica or shard to move, the source and target nodes, and the volume, and the plan respects every container's anti-affinity, media, and locality constraints.
2. **Given** a rebalancing plan in progress, **When** it is inspected, **Then** it reports what has moved, what is in flight, what remains, the current rate, and the estimated completion, and containers being moved continue to serve reads and writes at their declared quorum levels throughout.
3. **Given** a rebalancing operation, **When** the operator changes its rate limit, pauses it, or resumes it, **Then** the change takes effect without corrupting an in-flight move, and a paused plan remains valid and resumable.
4. **Given** a node being decommissioned, **When** the operation completes, **Then** no container has a replica on that node, every affected container satisfies its declared factor and constraints, and the node can be removed from the topology; **When** the operation cannot complete because no target satisfies some container's constraints, **Then** the decommission stops short, and the blocking containers and constraints are named.
5. **Given** a node whose `az` label is corrected, **When** the change lands, **Then** every container whose anti-affinity depended on that key is re-evaluated, violations are reported, and the repair is planned under the same rules as any other rebalance.
6. **Given** a rebalance interrupted by a node failure or a restart of the coordinating controller, **When** the cluster recovers, **Then** the plan resumes from its recorded state rather than restarting, and no data is lost or duplicated by the interruption.
7. **Given** a completed rebalance, **When** every container is described, **Then** each reports its constraints satisfied, its replicas in sync, and no residual copies on nodes outside its placement.

---

### Edge Cases

- Replication factor larger than the number of nodes, or larger than the number of distinct values of the anti-affinity label: refused at declaration naming both numbers; never placed with two replicas sharing a domain value "for now".
- Anti-affinity by a label key that some nodes do not carry at all: nodes missing the key are excluded from placement for that container, and the exclusion and its reason appear in the placement report; an unlabelled node is never treated as its own anonymous domain.
- All nodes in a domain (an AZ) are relabelled into another domain, collapsing a placement's distinctness after the fact: the placement is reported as violated, traffic continues, and the repair goes through rebalancing; existing data is never dropped to restore the invariant.
- Two containers with conflicting constraints competing for the last node with matching media: placement is first-come, and the loser is reported as unplaceable with the capacity that would satisfy it, rather than evicting the winner.
- A node declares a memory pool smaller than the memory-mode containers already placed on it after a restart: the containers that fit are restored, the rest are reported as unplaceable on that node with the shortfall, and they are re-placed by rebalancing.
- Quorum level stated in a protocol that has no field for it and whose option mechanism the client did not use: the container's declaration applies, and failing that the global default; the applied source is always reported so nobody has to guess.
- Write quorum satisfied but a replica acknowledges and then fails before persisting: the write is durable by the definition of a counted acknowledgement. For persistent and hybrid containers only drive-backed acknowledgements count; a memory-only acknowledgement is reported and does not satisfy write quorum. For memory-mode containers a memory acknowledgement does count, and the write is lost if every replica that acknowledged it restarts before replicating further. The container's description states which replicas can make a durable acknowledgement.
- Quorum arithmetic on an even replication factor: the strict-majority rule is documented once with a worked example for factors 2, 3, 4, and 5 so that `QUORUM` on factor 2 is understood to mean both replicas.
- Read at a level that contacts one replica while an asynchronous write is still in flight: the read may return the older value, this is the documented consequence of asynchronous replication with a read level below quorum, and the staleness bound is reported rather than the result being presented as current.
- A level that requires a remote acknowledgement (`EACH_QUORUM`, or `ALL` when a destination group is asynchronous) on a container whose remote replication is asynchronous: refused unless the container has declared that such requests wait; default is refuse, so a local write stays local.
- Synchronous replication declared to a destination that is unreachable for a long time: the container's declared behaviour governs — either writes fail while the destination is required, or it is demoted to asynchronous under a documented rule — and whichever applies is visible in the description rather than silently chosen.
- Asynchronous replication backlog exceeding the retention the source can hold: the stream switches from replaying missed writes to a full comparison against the source, and this is reported as it happens, not discovered later as a gap.
- A shard whose key distribution puts nearly all data in one shard: reported as imbalance with the measured skew; the cluster does not silently re-key the container, since changing a sharding key is a migration.
- A query that spans shards where one shard cannot meet the quorum: the whole query fails naming the shard and the level, unless the client explicitly asked for partial results; a partial answer is never returned as complete.
- Distributed transaction spanning shards or regions where one participant becomes unreachable mid-commit: the commit protocol resolves to a single outcome for every participant, the outcome is durable, and no participant is left indefinitely undecided without that being visible.
- Placement declared for a container whose type the catalog marks incompatible with that capability: refused by the type system before it reaches placement; this feature never receives an incompatible declaration.
- A node running a release that does not know a container's type while placement would otherwise select it: the node is excluded from that container's placement, and the exclusion reason names the version difference.
- Clock skew between nodes affecting last-writer-wins: the version stamp's dependence on time is documented, the skew is measured and reported, skew beyond the documented tolerance is a cluster health problem, and a type-supported alternative merge that does not depend on time remains available for containers that declare it.
- Every replica of a memory-mode container restarts at once: the content is gone, the containers are restored empty as the type system defines, and the cluster reports content loss explicitly rather than reporting an empty container as healthy.
- Rebalancing that would, at some intermediate step, put two replicas in the same anti-affinity domain: the plan is ordered so the invariant holds at every step, or, where a temporary violation is unavoidable, it is declared in the plan before it happens and bounded in time.

## Requirements *(mandatory)*

### Functional Requirements

**Topology: nodes, labels, drives, memory**

- **FR-001**: Every node MUST be describable with a set of labels as key/value pairs. The product reserves the near→far keys `rack`, `az`, `region`, `continent`, `planet`. Operators MUST NOT reuse those names for non-topology meaning (media stays on drive/memory labels). A cluster MUST declare an ordered **topology ladder**: an ordered subsequence of that reserved list (unused rungs MAY be omitted) and MAY append extra custom keys after the reserved names it uses. Starter ladders: first binary / one room `[az]` (or `[rack]`); production region `[rack, az, region]`; planetary `[az, region, continent, planet]`. **Demanded fill-in is per cluster, not per product.** Every member node MUST supply a value for every key on that cluster's ladder. Omit → join refused (`11`) or the node MUST NOT become `ready` as a replica target. Custom keys (`provider`, `jurisdiction`, `power_feed`, …) are allowed and MUST NOT be treated as farness unless the operator puts them on the ladder. Residency is a constraint (`region=eu` or `jurisdiction=…`); do not overload `planet` for GDPR. **`planet` is a label, never an HLC or write-quorum domain.** The ladder MUST NOT be inferred as a `quorum_domain` (`12`).
- **FR-001a**: **Hierarchy integrity:** if two nodes share a value on a finer ladder key, they MUST share all coarser keys on the ladder. `az=a1` with two different `region` values is a config error (refuse the join or the label change).
- **FR-001b**: Replica choice and shuffle locality (`05`) MUST rank candidate nodes by the **first ladder key that differs**. **Measured internode RTT** (and HLC skew as a health signal, `12`) MUST override that rank once samples exist. A missing ladder value is not “local”.
- **FR-002**: Node labels MUST be part of cluster metadata: every node MUST resolve every other node's labels identically, and a label change MUST propagate to the whole cluster.
- **FR-003**: Every drive a node offers for storage MUST be declarable with at least a media kind (for example `nvme`, `ssd`, `hdd`), a capacity, and its own labels. The media kind inventory MUST be expandable without changing existing declarations.
- **FR-004**: Drive properties MUST enrich the node's label set so that a placement constraint can select nodes by the media they offer, and the cluster MUST report both the node's declared labels and the labels derived from its drives, distinguishing the two.
- **FR-005**: In-memory storage MUST be declared explicitly per node with a size and its own labels. A node MAY declare no memory pool; nodes without one MUST be excluded from placements requiring memory, with that exclusion reported as the reason.
- **FR-006**: The cluster MUST publish a topology view listing every node with its labels, its drives (media, capacity, free space, labels), its memory pool (size, labels, usage) if any, and its current status; the view MUST be readable from any node and MUST be consistent across nodes.
- **FR-007**: The cluster MUST report, per label key, the number of distinct values present (the placement domains of that key), so that the maximum replication factor achievable with anti-affinity by that key is knowable before a container is declared.
- **FR-008**: Topology declarations MUST be validated: duplicate label keys on one node, a drive with no media kind or an unreadable path, a memory pool without a size or exceeding what the node can provide, and a node name colliding with an existing node MUST each be refused with an error naming the offending element; a rejected declaration MUST NOT be partially applied.
- **FR-009**: Adding, changing, or removing a node label, adding or removing a drive, and adding, resizing, or removing a memory pool MUST be possible on a running node without stopping it, and each change MUST trigger re-evaluation of every placement whose constraints reference the changed property.
- **FR-010**: Every free-capacity value the topology reports (per drive, per memory pool, per node) MUST be current enough to be used as a placement input, and placement MUST NOT select a target whose reported free capacity cannot hold the placed data.

**Placement constraints**

- **FR-011**: A container MUST be able to declare a placement constraint as an expression over node, drive, and memory labels, supporting at least equality, inequality, set membership, and key presence, combined with conjunction and disjunction.
- **FR-012**: Placement MUST select only nodes (and, within a node, only drives or memory pools) that satisfy every constraint the container declares, and MUST produce a placement report naming the selected targets, the constraints each satisfies, and, for every candidate excluded, the specific reason for exclusion.
- **FR-013**: A constraint that no current topology can satisfy MUST cause the declaration to be refused before any storage is allocated, with an error naming the unsatisfied constraint, what the topology offers, and what would satisfy it. A container MUST NOT be placed in violation of a declared constraint.
- **FR-014**: `persistent placement` MUST be expressible: a container MUST be able to pin replicas to a stated label set (for example a region) so that no automatic action — rebalancing, failure repair, or capacity pressure — moves them outside it; if the pinned set cannot host the container, it MUST be reported as unplaceable rather than placed elsewhere.
- **FR-015**: A container MUST be able to combine media, locality, and anti-affinity constraints, and all of them MUST hold simultaneously for every replica.
- **FR-016**: Placement MUST be deterministic in its inputs: given the same topology, constraints, and existing placements, every node MUST compute the same placement decision, and the decision MUST be recorded in cluster metadata rather than recomputed differently per node.
- **FR-017**: Placement MUST balance load across satisfying candidates — data volume and request load MUST be placement inputs, not only constraint satisfaction — and the balancing rule MUST be documented.
- **FR-018**: Every placement decision MUST be inspectable after the fact: which nodes hold the container, when the placement was made or last changed, why it changed, and whether it currently satisfies every declared constraint.

**Replication and anti-affinity**

- **FR-019**: A container MUST be able to declare a replication factor, and the cluster MUST maintain that many copies, each on a distinct node.
- **FR-020**: A container MUST be able to declare anti-affinity by one label key or by an ordered list of keys; no two replicas MAY share a value of the selected key, and with an ordered list the spread MUST be maximised at the first key, then the next within it, and so on. The satisfied level MUST be reported per replica. **Default anti-affinity for RF≥2** MUST be the **finest** key on the cluster ladder (usually `az` or `rack`). The operator MAY name a coarser key. If the topology cannot provide enough distinct values, placement is **refused** — no silent downgrade.
- **FR-021**: A replication factor that exceeds the number of nodes, or that exceeds the number of distinct values of the anti-affinity key, MUST be refused at declaration naming the required and available counts. Anti-affinity MUST NOT be silently downgraded, MUST NOT be placed as a weaker spread, and MUST NOT be accepted as a degraded-but-created container. A best-effort spread is out of scope for this feature.
- **FR-022**: Nodes that do not carry the anti-affinity label key MUST be excluded from placement for that container, and the exclusion MUST be reported; a missing label MUST NOT be treated as a distinct domain value.
- **FR-023**: A container MUST be able to declare a per-label-value replica distribution (for example three replicas in one region and two in another) instead of, or in addition to, a single factor, and the cluster MUST maintain each stated count independently.
- **FR-024**: A container's description MUST report, per replica: the node, that node's labels, the drive or memory pool holding it, the replication mode governing it, whether it is in sync, its lag if asynchronous, and its health.
- **FR-025**: Raising a replication factor MUST place and populate new replicas in the background while the container serves traffic; lowering it MUST remove surplus replicas only after the remaining set satisfies every declared constraint.
- **FR-026**: A placement whose anti-affinity or other constraint is violated after the fact (by a label change, a node loss, or a decommission) MUST be reported as degraded with the violated constraint named, MUST keep serving, and MUST be repaired through the rebalancing capability; data MUST NOT be dropped to restore an invariant.
- **FR-027**: A container declared with no replication capability MUST be placed as a single copy under the cluster's documented default placement, and its description MUST state that it has one copy and no anti-affinity guarantee.

**Quorum and consistency**

- **FR-028**: The cluster MUST support a per-query quorum level in the Cassandra-style vocabulary shared with the protocol drivers feature: `ONE`, `TWO`, `THREE`, `QUORUM`, `LOCAL_QUORUM`, `EACH_QUORUM`, `LOCAL_ONE`, `ALL`, and an explicit acknowledgement count. This feature owns **which nodes are replica targets** (RF, labels, anti-affinity). **Which of those targets count** for a given write or read, including `LOCAL_*` and `EACH_QUORUM`, is `12`. Write `ONE` / `LOCAL_ONE` / `TWO` / `QUORUM` count only durable replicas in the container's **source `quorum_domain`**. Log-followers never count toward those write levels.
- **FR-029**: The global defaults MUST be: a write waits for acknowledgement from two replicas, and a read is satisfied by one. These defaults MUST apply whenever no level is supplied and MUST be configurable per node and reportable with their source.
- **FR-030**: A protocol that has no native quorum field MUST be able to carry the level through its option mechanism; when nothing is supplied by any means, the container's declaration MUST apply, and failing that the global default. Precedence MUST be: query, then session, then container, then namespace, then global.
- **FR-031**: A container MUST be able to declare its own default read and write quorum levels, which override the global defaults for requests that state nothing.
- **FR-032**: `QUORUM` (write) MUST mean a strict majority of **durable source-domain** replicas (`12`). `LOCAL_*` MUST name the **coordinator's `quorum_domain`**, not “same AZ” and not a ladder key. `LOCAL_*` **write** on a follower still requires the corresponding durable ack **in the source**. `LOCAL_*` **read** on a follower MAY be served from a local applied replica and MAY be stale. `EACH_QUORUM` MUST mean required quorum in the source **plus** apply of that log position in each opted-in follower domain (`12`). `ALL` MUST mean every replica the container declared, including followers, and is subject to FR-074 when followers are async. This feature MUST NOT treat the topology ladder as a `quorum_domain`.
- **FR-033**: A quorum level that the container's replication factor cannot satisfy MUST be rejected before execution when it was stated explicitly, and clamped to the highest satisfiable level when it came from a default; a clamped value MUST be reported together with what it was clamped from.
- **FR-034**: A request whose quorum cannot be achieved at execution time because replicas are unavailable MUST fail with an error naming the required level, the acknowledgements achieved, and the unavailable replicas. A write that did not reach its level MUST NOT be reported as successful. For persistent and hybrid containers, only acknowledgements from drive-backed replicas MUST count toward write quorum; a memory-only replica MAY be contacted and MUST be reported, but MUST NOT satisfy the count. For memory-mode containers, acknowledgements from memory replicas MUST count. The execution record MUST distinguish durable acknowledgements from non-durable ones.
- **FR-035**: Every executed request MUST report the applied quorum level, its source, any clamping, the coordinating node, the replicas contacted, the acknowledgements received, and which of those acknowledgements were durable.
- **FR-036**: Reads and writes at levels whose intersection guarantees it (for example `QUORUM` write followed by `QUORUM` read) MUST return the last acknowledged value; the cluster MUST document, per level pair, whether read-your-write and monotonic reads hold.
- **FR-037**: When replicas of the same key disagree, the default rule MUST be last-writer-wins by a per-write version stamp: every replica MUST converge on the value with the later stamp, the disagreement MUST be recorded, and stale replicas MUST be corrected. A container MAY declare a type-supported alternative (for example an order-insensitive merge) where the type's descriptor allows it; that alternative MUST also be deterministic and recorded. Clock skew MUST be measured and reported, and skew beyond the documented tolerance MUST be a cluster health problem. Concurrent writes MUST NOT be retained as unresolved conflicts waiting for a client.

**Synchronous and asynchronous replication**

- **FR-038**: Replication MUST be declarable as synchronous or asynchronous, and the mode MUST be declarable per destination group (for example synchronous within a region, asynchronous to another region) rather than only per container.
- **FR-039**: Synchronous replication MUST implement quorum on writes: the coordinator MUST wait until the write's quorum level is met by synchronous replicas whose acknowledgements count under FR-034 before acknowledging the client.
- **FR-040**: Asynchronous replication MUST implement quorum on reads: correctness for an asynchronously replicated container MUST be obtained by the read contacting enough replicas, and the cluster MUST document which read levels give which guarantee for a given replication mode.
- **FR-041**: Asynchronous replication MUST expose its state: outstanding backlog, lag in time and in pending writes, a configurable lag threshold, and a health status derived from it. A read served from a lagging replica MUST be able to report the staleness bound that applies.
- **FR-042**: When an asynchronous backlog exceeds what the source retains, the destination MUST be brought up to date by a full comparison against a healthy replica instead of by replaying missed writes, and the switch MUST be reported.
- **FR-043**: A synchronous destination that becomes unreachable MUST be handled by a rule the container declares — either writes requiring it fail, or it is demoted to asynchronous under a documented condition — and the rule in effect MUST appear in the container's description. The cluster MUST NOT choose silently.
- **FR-044**: Replication behaviour MUST be derived from the container's datatype and composition as declared through the type system: the replication unit, what an acknowledgement means for that type, and how conflicts are resolved MUST come from the type's descriptor, and no replication path MAY bypass the abstract type interface.

**Any node coordinates**

- **FR-045**: Every **member** node MUST accept a request for any container in the cluster, whether or not it holds a replica, and MUST coordinate it: resolve placement, route to the shard and replicas, apply the quorum, and answer. The data path MUST be **leaderless in the source `quorum_domain`**: any replica **in that domain** MUST be able to accept a write; the coordinator waits for quorum acknowledgements **in that domain**. Writes that land on a log-follower MUST **forward to the source**; the follower MUST NOT open an independent source log. Ordered/log types MUST NOT accept independent writes on a follower. No replica MAY be a mandatory write serializer **inside the source**. The control plane's datatype-level primary MUST own shared-datatype metadata, not per-write serialization.
- **FR-046**: The result of a request MUST NOT depend on which node received it; only latency MAY differ.
- **FR-047**: A coordinating node MUST resolve placement from cluster metadata that is consistent cluster-wide, and MUST refuse rather than guess when its view of placement is known to be stale.
- **FR-048**: The coordinating node MUST be recorded in the execution record of every request, together with the replicas it contacted.
- **FR-049**: Loss of a coordinating node MUST NOT make a container unreachable: a client MUST be able to retry through any other node, and the container's description MUST state which of its operations are safe to retry.

**Sharding and partitioning**

- **FR-050**: A container MUST be able to declare a sharding key and a shard scheme so its data is divided across several replica sets, with every shard placed independently under the container's replication factor, anti-affinity, media, and locality constraints.
- **FR-051**: A container MUST be able to declare a partitioning scheme (at least by key range and by time) so its data is divided into independently placed partitions, and partitions MUST be able to carry different media or locality constraints from one another while presenting one container to clients.
- **FR-052**: Every key MUST resolve to exactly one shard and one partition at any moment, from any node, including while a shard or partition boundary is being changed.
- **FR-053**: A query that spans several shards or partitions MUST be fanned out by the coordinating node and combined; the quorum MUST be applied per shard; and a shard that fails to meet it MUST fail the query naming the shard and the level, unless the client explicitly requested partial results.
- **FR-054**: Changing the shard count or a partition boundary MUST move the affected data in the background with the container still serving, MUST NOT lose or duplicate any acknowledged write, and MUST keep every key resolvable throughout.
- **FR-055**: A sharding or partitioning declaration that the container's type does not support, that conflicts with another declaration, or that names a key absent from the container's schema MUST be refused naming the conflict, with nothing placed.
- **FR-056**: Per-shard and per-partition data volume and request rate MUST be observable, and a shard exceeding a documented imbalance threshold MUST be reported as a rebalancing candidate.

**Failure handling and repair**

- **FR-057**: The cluster MUST detect that a node or a replica is unreachable within a configurable interval and MUST mark the affected placements as degraded, naming the failure and the constraints at risk.
- **FR-058**: While a replica is unavailable, requests at levels the remaining replicas can satisfy MUST keep succeeding, and requests at levels they cannot MUST fail explicitly (FR-034).
- **FR-059**: Writes that an unavailable replica missed MUST be retained for a configurable window and applied when it returns, without operator action; the amount outstanding MUST be visible while it catches up, and a replica MUST NOT be reported as in sync until it is.
- **FR-060**: When the retention window is exceeded or a replica's state cannot be reconciled by replay, the replica MUST be brought up to date by a full comparison against a healthy replica, with the difference reported.
- **FR-061**: Disagreement detected while serving a read MUST be resolved for the client by the container's conflict-resolution rule and MUST trigger correction of the stale replicas in the background.
- **FR-062**: The cluster MUST run a background consistency check between replicas at a configurable rate, MUST report what it finds and repairs, and MUST be throttleable so that it does not displace client traffic.
- **FR-063**: During a network partition, a side that cannot reach the acknowledgements a level requires MUST refuse the request rather than accept it locally; on heal, the cluster MUST converge by the documented conflict-resolution rule and MUST report the conflicts it resolved.
- **FR-064**: Permanently removing a node MUST re-place every container that had a replica on it onto nodes satisfying its constraints, MUST show progress, and MUST restore each container's declared factor; a container for which no satisfying target exists MUST be named as blocking rather than silently left under-replicated.

**Rebalancing**

- **FR-065**: Adding nodes, removing nodes, changing node labels, changing a replication factor, and changing a placement constraint MUST each produce a rebalancing plan naming every replica, shard, or partition to move, its source and target, and the volume involved.
- **FR-066**: A rebalancing plan MUST respect every container's constraints at every step; where a temporary violation is unavoidable, the plan MUST declare it in advance and bound its duration.
- **FR-067**: Rebalancing MUST run in the background while containers keep serving reads and writes at their declared quorum levels, and it MUST be rate-limited by a control the operator can change while it runs.
- **FR-068**: A rebalancing operation MUST be inspectable (moved, in flight, remaining, rate, estimate), pausable, and resumable, and MUST resume from its recorded state after a node or controller failure rather than restarting.
- **FR-069**: Rebalancing MUST NOT lose or duplicate acknowledged data, and the container MUST resolve every key to exactly one location throughout the move.
- **FR-070**: On completion, every affected container MUST report its constraints satisfied, its replicas in sync, and no residual copies outside its placement.

**Multi-region and planetary scale**

- **FR-071**: Farness for **planning and replica ranking** MUST use the topology ladder (FR-001b). "Local" in `LOCAL_QUORUM` and `LOCAL_ONE` MUST mean the coordinator's **`quorum_domain`** (`12`), not a ladder key. Destination groups for sync vs async replication MUST be declared as source vs log-follower domains, not inferred from `planet` or `region`.
- **FR-072**: A local write at a local level MUST NOT wait on a remote destination whose replication mode is asynchronous.
- **FR-073**: No timeout, lag threshold, retry budget, failure-detection interval, or repair interval MAY be fixed at a value that assumes a low round trip; every one of them MUST be configurable per cluster and, where it applies to a destination group, per group.
- **FR-074**: `EACH_QUORUM` and any other level that requires an acknowledgement from an asynchronous destination group MUST be refused by default, with an error naming the level, the asynchronous group, and that the container must declare waiting to honour the level. A container MAY opt in so that those requests wait for the remote acknowledgement. The behaviour in effect MUST appear in the container's description. The cluster MUST NOT wait by surprise and MUST NOT treat a queued remote send as an acknowledgement.
- **FR-075**: The documentation MUST include working starter examples for at least: a single node; a single rack; three availability zones in one region; two regions with synchronous local and asynchronous remote replication; a mixed-media cluster; and a topology whose inter-group round trip is measured in minutes. Each example MUST work verbatim and MUST state the resulting placement, replication modes, and quorum behaviour.

**Distributed transactions, consensus, and leader election**

- **FR-076**: A container MUST be able to declare the distributed transactions capability, and the cluster MUST provide an atomic commit across the participants of one transaction such that every participant reaches the same outcome and that outcome is durable.
- **FR-077**: A participant that becomes unreachable mid-commit MUST NOT leave the transaction undecided without that state being visible and eventually resolved; the resolution rule and its bound MUST be documented.
- **FR-078**: Where a container's type or capability declaration requires a leader (for example an ordered replication stream or a single-writer datatype), leadership MUST be established by the control plane's election mechanism, MUST be reported in the container's description, and MUST fail over without operator action. That leadership is the exception, not the default: a container without such a declaration MUST remain leaderless per FR-045. Data-path reads and writes MUST remain available from every node; leadership MUST NOT become a mandatory proxy for ordinary replicated containers.
- **FR-079**: Consensus and leader election MUST NOT be reimplemented by this feature: it declares the need and consumes the control-plane mechanism, and the two MUST NOT produce contradictory views of who leads.

**Reporting and accounting**

- **FR-080**: Every container's placement, replication state, replication lag, degradation, and rebalancing progress MUST be readable from any node through the node's admin surfaces and bundled CLI.
- **FR-081**: The cluster MUST count and expose, per container and per node, the acknowledgements required and achieved per request, the requests refused for unachievable quorum, the replication lag, the repair volume, and the rebalancing volume, so the observability feature can publish them and the tenancy feature can attribute them.
- **FR-082**: Every refusal this feature produces (unsatisfiable constraint, unsatisfiable or unachieved quorum, unplaceable container, blocked decommission) MUST name the container, the constraint or level involved, the current topology facts that caused it, and what would resolve it.
- **FR-083**: Placement, replication, and quorum configuration MUST be reportable as effective configuration with the source of each value (query, session, container, namespace, cluster default, built-in default).

### Key Entities

- **Node**: A cluster member with a name, a set of labels, zero or more drives, an optional memory pool, and a status. Every node is a potential coordinator for every container.
- **Label**: An operator-defined key/value pair on a node, a drive, or a memory pool. Declared labels and labels derived from drives are distinguished. Reserved farness keys are `rack`, `az`, `region`, `continent`, `planet` and appear only as the cluster **topology ladder** demands.
- **Topology Ladder**: The cluster's ordered subsequence of reserved farness keys (plus optional custom keys after them). Member nodes fill every ladder key. Not a `quorum_domain`.
- **Placement Domain**: The set of distinct values of one label key across the cluster. Its cardinality is the maximum number of replicas that can be spread apart by that key.
- **Drive**: A declared storage device on a node with a media kind (`nvme`, `ssd`, `hdd`, expandable), capacity, free space, and labels; contributes derived labels to its node.
- **Memory Pool**: A node's explicitly declared in-memory storage with a size, labels, and usage. Optional per node; required for memory-mode placement.
- **Topology View**: The cluster-wide, consistent picture of nodes, labels, drives, memory pools, placement domains, and status, readable from any node.
- **Placement Constraint**: A label expression a container declares (media, locality, key presence, and combinations) that every replica must satisfy.
- **Placement**: The recorded decision of which nodes, drives, and memory pools hold a container's replicas, shards, and partitions, with the reason for each selection and each exclusion, and its current satisfied/degraded/unplaceable state.
- **Persistent Placement**: A pin that forbids automatic relocation of a container's replicas outside a stated label set; an unsatisfiable pin yields unplaceable rather than relocation.
- **Replica**: One copy of a container (or of a shard or partition of it) on one node, with a replication mode, a sync state, a lag, and a health status.
- **Replica Set**: The replicas of one container, shard, or partition, over which quorum is computed.
- **Replication Factor**: The declared number of replicas, expressible as a single number or as counts per label value.
- **Anti-Affinity Rule**: One label key or an ordered list of keys across whose values replicas must be spread; violation is reported, never silently accepted.
- **Replication Mode**: Synchronous or asynchronous, declarable per destination group; synchronous implements write quorum, asynchronous implements read quorum.
- **Destination Group**: A set of replicas selected by a label expression that shares one replication mode, lag threshold, and timing configuration — the unit of "local versus far".
- **Quorum Level**: `ONE`, `TWO`, `THREE`, `QUORUM`, `LOCAL_QUORUM`, `EACH_QUORUM`, `LOCAL_ONE`, `ALL`, or an explicit acknowledgement count; resolved by the precedence query → session → container → namespace → global, with defaults of two acknowledgements for writes and one for reads.
- **Locality Domain**: Obsolete name. Use **`quorum_domain`** (`12`) for voting/HLC/write-ack sets, and the **topology ladder** for farness ranking. `LOCAL_*` is the coordinator's `quorum_domain`.
- **Coordinator**: The node that received a request and drives it: resolving placement, routing writes to every eligible replica independently, counting acknowledgements, and answering. Any node, replica or not. Not a write leader.
- **Shard**: A division of a container by a sharding key, placed as its own replica set with the container's constraints.
- **Partition**: A division of a container by range or time, placed independently and able to carry its own media or locality constraints.
- **Conflict-Resolution Rule**: The deterministic rule by which disagreeing replicas converge. Default is last-writer-wins by a per-write version stamp; a container may declare a type-supported alternative. Concurrent writes are not left unresolved.
- **Replication Lag**: The measured distance of an asynchronous replica from its source, in time and in pending writes, with a configurable threshold and a derived health status.
- **Repair**: Bringing a replica up to date, either by applying retained missed writes or, beyond the retention window, by full comparison against a healthy replica.
- **Rebalancing Plan**: The named set of moves that restores every container's declared constraints after a topology or declaration change; inspectable, rate-limited, pausable, resumable, and constraint-safe at every step.
- **Execution Record**: The per-request account of the coordinating node, replicas contacted, quorum level applied, its source, any clamping, acknowledgements received, and which of those acknowledgements were durable.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A cluster architect following the starter documentation brings up a three-AZ cluster with labelled nodes, mixed media, and declared memory pools, and reads back a topology view matching the example, within 30 minutes on fresh machines.
- **SC-002**: 100% of the starter examples in FR-075 apply verbatim and produce the placement, replication modes, and quorum behaviour they state, in every conformance run.
- **SC-003**: For every (replication factor, anti-affinity key) pair the topology can satisfy, 100% of placements put replicas in distinct domain values; for 100% of unsatisfiable pairs the declaration is refused before any storage is allocated, with the required and available counts named.
- **SC-004**: 100% of media-constrained containers are placed only on matching drives or memory pools, and 100% of placements that no node satisfies are reported as unplaceable with the excluded candidates and their exclusion reasons, in every conformance run.
- **SC-005**: For every quorum level in the vocabulary, the acknowledgements required and the replicas contacted match the level's definition in 100% of conformance requests, measured from the execution record; a write with no stated level waits for exactly two counted acknowledgements (drive-backed for persistent and hybrid containers) and a read with no stated level contacts exactly one replica in 100% of cases.
- **SC-006**: 100% of requests stating a level the replication factor cannot satisfy are rejected before execution, and 100% of default-sourced levels in the same situation are clamped with the clamping reported.
- **SC-007**: Identical requests issued at a replica-holding node and at a node holding no replica return identical results in 100% of conformance cases, and every execution record names its coordinating node.
- **SC-008**: With one replica of three unavailable, 100% of requests at `ONE`, `TWO`, and `QUORUM` succeed and 100% at `ALL` fail with the unavailable replicas named; when the replica returns, 0 acknowledged writes are lost and the replica reports in sync within the configured catch-up bound.
- **SC-009**: Across the partition and heal scenarios of the conformance suite, 0 writes are acknowledged on a side that could not reach the required acknowledgements, and 100% of resolved conflicts are reported.
- **SC-010**: For a container with a source `quorum_domain` and asynchronous log-followers, the measured source-domain write latency at a source write level stays within the source round trip and includes 0 follower round trips, in 100% of sampled requests; asynchronous lag is reported continuously in time and pending writes.
- **SC-011**: In a sharded container, 100% of keys resolve to exactly one shard from every node, including throughout a shard-count change, and 0 acknowledged writes are lost or duplicated by the change.
- **SC-012**: A multi-shard query where one shard cannot meet its quorum fails naming that shard in 100% of cases and returns a partial result in 0 cases unless partial results were explicitly requested.
- **SC-013**: Adding a node, decommissioning a node, and relabelling a node each complete with 100% of affected containers reporting their constraints satisfied, 0 residual copies outside their placements, and 0 intervals in which a container was unavailable at its declared quorum level.
- **SC-014**: A rebalancing operation interrupted by a node or controller failure resumes from its recorded state in 100% of conformance runs, with 0 data loss or duplication.
- **SC-015**: 100% of distributed transactions in the conformance suite, including those with a participant failed mid-commit, resolve to a single durable outcome agreed by every participant, with 0 participants left undecided past the documented bound.
- **SC-016**: 100% of refusals produced by this feature name the container, the constraint or level, the topology facts that caused the refusal, and what would resolve it.
- **SC-017**: Placement state, replication lag, repair volume, rebalancing progress, and per-request acknowledgement counts are available to the observability feature for 100% of containers and requests exercised in the conformance suite.
- **SC-018**: 100% of the capability declarations the type system passes to this feature (replication, sharding, partitioning, node placement, failure handling, rebalancing, consistency, persistent placement, labels/placement constraints, distributed transactions, consensus, leader election, quorum) are either executed with the semantics specified here or refused with a named reason; 0 are accepted and ignored.

## Assumptions

- **Division of labour with the type system (`003`)**: the type system owns the declaration, validation, and compatibility of the thirteen L1 shared capabilities and hands accepted declarations to this feature; this feature owns what they mean and does the placing, copying, coordinating, and repairing. An incompatible declaration never reaches this feature. Declarations are made at container level and cover every component of the container's layout as one unit.
- **Quorum vocabulary and precedence** are shared with the protocol drivers feature (`002`). This specification owns replica targets (RF, labels, anti-affinity) and hands counting rules to `12`. The global defaults (write two acknowledgements, read one) come from the constitution and apply **inside the source domain**.
- **Replica-set model**: the data path is leaderless (confirmed in Clarifications, Session 2026-09-14). Every node coordinates; any replica accepts a write; the coordinator waits for quorum acknowledgements; no replica is a mandatory write serializer. The control plane's datatype-level primary (`06`) owns shared-datatype metadata and leadership, not per-write serialization. Where a datatype genuinely requires a single writer or an ordered stream, leadership is requested from the control plane's election mechanism (FR-078).
- **Conflict resolution**: last-writer-wins by a per-write version stamp (confirmed in Clarifications, Session 2026-09-14), as in the Cassandra model the intent names, with a per-container alternative where the datatype supports one (for example order-insensitive merge). Clock skew is measured and reported; concurrent writes are not retained as client-resolved conflicts.
- **Durability of an acknowledgement**: persistent and hybrid containers count only drive-backed acknowledgements toward write quorum; memory-mode containers count memory acknowledgements (confirmed in Clarifications, Session 2026-09-14). A counted acknowledgement from a persistent replica means the write is recoverable on that node; a counted memory acknowledgement does not survive that node's restart. Mixed replica sets report which acknowledgements were durable. The memory-mode volatility rule itself is the type system's (`003`).
- **Anti-affinity is strict**: a factor that the topology cannot spread is refused rather than best-effort placed (confirmed in Clarifications, Session 2026-09-14). After-the-fact violations (label change, node loss) remain degraded-and-repaired per FR-026; that is recovery, not a creation-time downgrade.
- **Drive labels enrich node labels** rather than replacing them: a constraint naming media matches a node that has at least one matching drive, and the placement then selects that specific drive. The media kind vocabulary is expandable in the same spirit as the type inventories.
- **Locality vs farness**: the topology ladder ranks placement and shuffle (`05`); `LOCAL_*` and write-ack arithmetic live in `12` (confirmed in Clarifications, Session 2026-09-15). `planet` is a label. `EACH_QUORUM` on a container with asynchronous remote replication is refused unless the container opts into waiting (confirmed in Clarifications, Session 2026-09-14).
- **Failure detection, catch-up retention, repair rate, and rebalancing rate** are configuration with documented defaults; the exact default values are planning decisions, constrained by FR-073's rule that none of them may assume a low round trip.
- **Rebalancing versus migration**: automatic movement to restore declared constraints is this feature; operator- or tenant-initiated movement between nodes or namespaces and any rewrite of data is the migration and transforms feature (`10`).
- **Distributed transactions**: this feature provides the cross-participant atomic commit and its guarantees; transaction syntax, isolation levels, and how a query is planned into participants belong to the query execution feature (`05`).
- **Consensus and leader election mechanics** are the control plane's (`06`); this feature declares the need and consumes them, and the two must not disagree about who leads.
- **Metric series names and labels** are the observability feature's (`08`); this feature guarantees the counts of FR-081 exist per container and per node. Per-tenant attribution and quotas are the tenancy feature's (`07`).
- **Configuration surface**: the `labels`, `storage`, `memory`, `cluster`, and `replication` directives are already reserved in the runtime configuration grammar (`001`) for this feature; their concrete syntax is a planning decision that must satisfy FR-001 through FR-010 and FR-083.
- **Conformance harness and the starter topology examples** of FR-075 are deliverables of this feature, consistent with the constitution's documented-configuration principle.

## Out of Scope

The intent file and its siblings assign the following elsewhere. This specification does not define them; it only reserves the touchpoints named above.

- **Raft election mechanics and the controller hierarchy** (intent `06`). This feature consumes elected leadership where a datatype requires it and never runs a second election protocol.
- **Cluster/node identity and join/leave/replace** (intent `11`) except that join MUST present ladder labels and hierarchy integrity.
- **Internode framing, HLC, `quorum_domain` membership, promote/fence, `multi_active`** (intent `12`) except the leaderless/LWW/forward rules and RTT-override rank stated here.
- **WAL/fsync meaning of ack** (intent `13`).
- **Query IR and shuffle** (intent `05`) except the farness rank contract (FR-001b).
- **Type inventories, capability compatibility, container schemas, storage modes, encodings, compression, and encryption** (intent `03`, feature `003`). This feature places and copies what that feature defines.
- **Query parsing, planning, scheduling, execution, joins, aggregation, and transaction syntax and isolation** (intent `05`). This feature provides placement resolution, per-shard quorum, and atomic commit across participants.
- **Per-namespace quotas, access policies, roles, and key management** (intent `07`). This feature provides per-container and per-node accounting.
- **Replication and placement metric series names** (intent `08`). This feature guarantees the underlying counts exist.
- **Cluster-map and topology visualisation** (intent `09`). This feature guarantees the topology and placement views it would render.
- **Migration between nodes or namespaces initiated by an operator or tenant, and any data transform** (intent `10`).
- **Protocol-level syntax for carrying quorum and timeout options** (feature `002`). This feature owns the semantics behind the values those options carry.
- **Best-effort anti-affinity** that places a container when the topology cannot spread replicas as declared. This feature refuses such declarations.

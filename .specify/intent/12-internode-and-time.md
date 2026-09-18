---
speckit_command: specify
suggested_slug: internode-and-time
source: gap-analysis-2026-09-14
read_after: 00-constitution.md
---

# Feature: Internode fabric, clocks, and replica conflict resolution

Specify the node-to-node channel (distinct from client protocols), the cluster clock, failure detection, and what happens when replicas disagree.

## What

Client protocols (`02`) are for applications. Cluster-internal traffic MUST use dedicated entrypoint handler(s), not a client protocol port:

- **`internode`**: membership gossip, Raft RPCs (`06`), failure detection, repair/anti-entropy coordination, query shuffle/data-exchange (`05`), distributed-transaction coordination (`04`/`05`).
- **`replication`**: streaming copy of container data between replicas (sync and async replication from `04`).

Each MUST be declared as an entrypoint with exactly one handler (`01`). They MUST NOT share a port with a client protocol or with `admin` / `admin-http`. **Both `internode` and `replication` MUST listen even on a single node with no peers.** Documented default **listen address is loopback**; a node MUST bind a **cluster address** on those entrypoints before it can `join` a remote cluster.

Every entrypoint MUST declare **`tls { ... }` or `plaintext;`**. Omitted transport is a **startup error** (`14`/`01`). TLS is not globally mandatory; accidental omit must not become silent plaintext. Internode peers MUST authenticate as the **replication role** (`07`/`14`); unauthenticated peers MUST be refused. Join secret (`11`) is required before the cluster protocol is spoken.

The internode protocol MUST be versioned. A node MUST refuse a peer whose internode version is outside the documented compatibility window (`15`: mixed-version N and N+1).

Backpressure: when a receiver's buffers are full, the sender MUST slow or fail the specific stream; it MUST NOT block the Tokio worker pool (`00`).

### Clocks

The cluster MUST use a **hybrid logical clock (HLC)** (physical time + logical counter) as the default version stamp for leaderless writes **inside one `quorum_domain`**. HLC values MUST NOT be compared across `quorum_domain`s. Wall-clock skew **inside** a domain MUST be measured and reported. Skew beyond the documented tolerance MUST be a cluster health problem (`node_state` degraded or equivalent), not silent reordering.

Types that need a total order (log stream, Raft-backed controller logs) MUST NOT use LWW; they MUST use the control-plane election / Raft log (`06`) as their order.

### Quorum domain (source vs log-follower)

A **`quorum_domain`** is a first-class cluster object: the set of nodes that share one **HLC** and one **synchronous write-quorum** set. Operators declare it explicitly. Label keys such as `region` or `planet` MUST NOT silently become a domain. **`planet` is never an HLC or write-quorum domain.** The cluster **topology ladder** (`04`) is farness for placement and planning, not a voting set. Internode RTT (and HLC skew) MUST be measured so the planner (`05`) can override ladder rank.

For each container there is **one source `quorum_domain` (A)**. Replicas in other domains (B, …) are **asynchronous log-followers**. They MUST NOT count toward write acknowledgements at `ONE`, `LOCAL_ONE`, `TWO`, or `QUORUM`.

**Write**

- A write received in A is handled in A (leaderless, or leader-for-ordered types).
- A write received in B is **forwarded to A** and creates a **source log entry in A**. B MUST NOT open an independent source log for data replicated from A.
- Ordered/log datatypes MUST NEVER accept independent writes in B (forward to the leader in A, or error).
- `LOCAL_ONE` **write** on B still requires **one durable WAL in A**. `LOCAL_` does not mean a local WAL on a follower.

**Ack**

- `ONE` / `LOCAL_ONE` write: one durable WAL copy in **A** (`13`).
- `TWO` / `QUORUM`: durable WAL copies **only in A**. B does not count.
- `EACH_QUORUM`: required quorum in A **plus** successful **apply of that log position in B**. If the container's remote is async without wait-for-apply, `EACH_QUORUM` is **refused** (`04`).

**Read**

- Reads in A follow the requested level in A.
- `LOCAL_ONE` **read** in B MAY be served from the local **applied** replica and MAY be stale. `LOCAL_*` for **reads** names the **coordinator's** domain.
- Stronger read consistency MUST **forward to A** when B cannot satisfy the level.

**Promote:** only **manual** `CLUSTER_ADMIN` promote of a follower domain: new **epoch**, that domain becomes source, the old source MUST NOT accept writes until it **rejoins as follower** (fence old epoch). Two live sources for the same container is protocol-refused.

**`multi_active`:** type-catalog flag, **default off** (KV, document, SQL). Ordered/log types **forced off**. Create with `multi_active=on` is **refused** in the first binary (`16`). Dual-active merge is not shipped until a non-HLC merge exists.

### Data-path replication model

The **data path is leaderless in the source `quorum_domain`**: any replica **in A** MAY accept a write; the coordinator waits for quorum acknowledgements **in A** (`04`). No replica is a mandatory write serializer **inside A**. Writes that arrive in a follower domain **forward to A** (see above).

The datatype-level primary (`06`) owns **shared-datatype metadata and leadership requests**, not per-write serialization. Where a datatype genuinely requires a single writer or an ordered stream, it MUST request leadership from the control plane.

### Conflict resolution

Cross-domain copy applies the **source log sequence**, not local HLC. Last **source position** wins on followers. Concurrent writes MUST NOT be retained as unresolved conflicts waiting for a client.

`EACH_QUORUM` (and any level that requires a remote acknowledgement) on a container with asynchronous remote replication MUST be refused unless the container declares that such requests wait (`04`).

When two replicas **in the same `quorum_domain`** hold different values, the default rule is **last-writer-wins by the HLC version stamp**. Every replica in that domain MUST converge on the value with the later stamp; the disagreement MUST be recorded; stale replicas MUST be corrected. A container MAY declare a type-supported alternative (for example an order-insensitive merge) where the type's descriptor allows it; that alternative MUST also be deterministic.

### Failure detection and partitions

Nodes MUST detect peer failure by heartbeat / timeout on the `internode` channel (not a Byzantine model). A suspected node is **unavailable for quorum counting** until it responds or is decommissioned (`11`).

The product consistency model is **tunable AP** in the Cassandra sense: availability of a request depends on the requested quorum versus live replicas, not on a single leader for data. Controller groups (`06`) are **CP** (Raft): a partitioned minority MUST NOT accept membership or schema changes.

Split-brain of **data inside a domain** is resolved by LWW/HLC (or the type's merge). Split-brain of **controllers** is resolved by Raft (minority does not elect). Two **source** domains for one container after a botched promote is **refused** (epoch fence). Two bootstraps with the same **name** are two clusters (`11`), not this feature.

Repair / anti-entropy, read repair, and hinted handoff MUST exist so quorum clusters do not drift forever; rate and retention are documented defaults (`04` owns placement repair; this feature owns the internode streams they use).

## Why

Clusters cannot share a client protocol for replication, cannot trust raw wall clocks across `quorum_domain`s, and cannot leave replica disagreement or follower-domain writes undefined.

## Actors

- Replica set streaming mutations over `replication`
- Raft controllers exchanging RPCs over `internode`
- Coordinator waiting for quorum acks from peers
- Operator inspecting clock skew and internode TLS
- Failure detector marking a peer unavailable

## Requirements

- Distinct `internode` and `replication` entrypoints **always listening**; versioned; replication role; explicit `tls` or `plaintext`; default bind loopback.
- Explicit `quorum_domain`; source vs log-follower write/ack/read/promote rules as specified. Topology ladder is not a domain (`04`).
- Internode RTT (and HLC skew) measured so the planner can override ladder rank (`05`).
- HLC only inside a domain; source log sequence across domains.
- Leaderless data path in the source domain; LWW-by-HLC in-domain; ordered types use leadership.
- `multi_active=on` refused in the first binary.

## Out of scope for this feature

- Membership bootstrap/join/leave (`11`)
- Placement RF, labels, and anti-affinity (`04`); this feature owns which replica targets **count** for a given write/read level
- What an acknowledgement means on disk (`13`)
- Client protocols (`02`)
- Key material and principal store (`14`)

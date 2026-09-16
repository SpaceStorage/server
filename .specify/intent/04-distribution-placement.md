---
speckit_command: specify
suggested_slug: distribution-placement
source: server/start
read_after: 00-constitution.md
---

# Feature: Distribution, placement, disks, memory, and replication

Specify how shared (L1) capabilities place and copy data across nodes, using labels, drive types, memory pools, and Cassandra-style quorum.

## What

Shared (L1) provides distributed capabilities that can be applied to compatible foundation primitives and higher-level abstractions:

- replication
- sharding
- partitioning
- node placement
- failure handling
- rebalancing
- consistency
- persistent placement
- labels / placement constraints
- distributed transactions
- consensus
- leader election
- quorum

All options for data MUST be documented well with examples to start, but stay flexible enough to create complex cluster systems in many regions, continents, or even planets **as label keys**, grouping datatypes with data near the user and replicating slowly to remote regions. **`planet` is a label, never an HLC or write-quorum domain.** The voting / HLC set is an explicit **`quorum_domain`** (`12`). Topology farness is a **cluster-declared ladder** of well-known keys, not a product-wide fill-in of every geographic name.

### Topology ladder (farness)

The product reserves this **near → far** order. Operators MUST NOT reuse these names for non-topology meaning (media stays on drive/memory labels):

`rack`, `az`, `region`, `continent`, `planet`

A cluster MUST declare an ordered **topology ladder**: an ordered **subsequence** of that reserved list (unused rungs may be omitted) and MAY append extra custom keys after the reserved names it uses. Starter examples:

- first binary / one room: `[az]` (or `[rack]`)
- production region: `[rack, az, region]`
- planetary: `[az, region, continent, planet]`

**Demanded fill-in is per cluster, not per product.** Every member node MUST supply a value for **every key on that cluster's ladder**. Omit → join refused (`11`) or the node MUST NOT become `ready` (degraded, not a replica target). A laptop is not forced to invent `planet` unless the ladder includes it.

**Hierarchy integrity:** if two nodes share a value on a finer ladder key, they MUST share all coarser keys on the ladder. `az=a1` with two different `region` values is a config error (refuse the join or the label change).

Custom keys (`provider`, `jurisdiction`, `power_feed`, …) are allowed. They MUST NOT be treated as farness unless the operator puts them on the ladder. **Residency is not farness:** pin with a constraint (`region=eu` or `jurisdiction=…`); do not overload `planet` for GDPR.

The ladder MUST NOT be inferred as a `quorum_domain`. Sync vs async remains `12`.

### Node labels and anti-affinity

Nodes carry the ladder labels plus any extra keys. Example for ladder `[rack, az, region]`:

```text
node1: {
    "rack": "rack1",
    "az": "az1",
    "region": "region1",
}

node2: {
    "rack": "rack2",
    "az": "az2",
    "region": "region1",
}
```

These two nodes are enough for **inter-AZ** replication, but do **not** fit **inter-region** replication.

Shared datatypes can be replicated with a specified replication factor and anti-affinity by a selected label (or an ordered list of keys: spread on the coarsest named key first, then finer). **Default anti-affinity for RF≥2** is the **finest** key on the cluster ladder (usually `az` or `rack`). The operator MAY name a coarser key. If the topology cannot provide enough distinct values, placement is **refused** — no silent downgrade.

Label selection MUST be documented with starter examples (single-node, single-rack, three-AZ, two-region, planetary). A single-node cluster MAY have one value on every ladder key.

### Farness for planning

Replica choice and shuffle locality (`05`) rank candidate nodes by the **first ladder key that differs** (same `az` before same `region` before same `planet`). **Measured internode RTT** (and HLC skew as a health signal, `12`) MUST override that rank once samples exist. A missing ladder value is not “local”. `LOCAL_*` remains the coordinator's **`quorum_domain`**, not “same AZ”.

### Drives and memory as labels

Each server can have different drives and types of drives: SSD, HDD, NVMe, etc. Disks MUST enrich node labels so various drives can be selected for different types of data.

In-memory storage: each node MUST specify explicitly memory size and labels for this type of storage. Different nodes can have different sizes of memory storage, or memory storage can be absent on some nodes.

### Quorum and who serves traffic

Storage and drivers MUST specify quorum level for a query like in Cassandra. Protocols that do not support this MUST add it via options or use defaults: write wait for two nodes (`ack == 2`), read from every node (`ack == 1`).

**Write quorum arithmetic** (`ONE` / `LOCAL_ONE` / `TWO` / `QUORUM`) counts only durable replicas in the container's **source `quorum_domain`**. Log-followers never count toward those write levels. `LOCAL_*` for **reads** is the **coordinator's** domain. `EACH_QUORUM` and write-forward from a follower domain are specified in `12`. This feature owns RF, labels, anti-affinity, and which nodes are replica targets; `12` owns which of those targets count for a given write.

Every node can receive requests from the user and operate with data.

### Sync vs async replication

Replication can be synchronous or asynchronous.

- Synchronous replication MUST implement quorum level for **write** operations.
- Asynchronous replication MUST implement quorum level for **read** operations.
- All replication logic is based on **datatypes and data composition**.

The **data path is leaderless in the source domain** (`12`): any replica **in that domain** may accept a write; the coordinator waits for quorum acknowledgements **in that domain**. Writes that land on a log-follower **forward to the source**. Conflict default **in-domain** is last-writer-wins by HLC stamp; type-supported merge optional. Cross-domain copy follows the **source log**. Durable vs memory acknowledgements are `13`. Membership of replica targets is `11`. Anti-entropy uses the `internode` / `replication` handlers (`12`). Catalog `multi_active` default off; create with on refused in the first binary (`12`, `16`).

`EACH_QUORUM` on a container with asynchronous remote replication MUST be refused unless the container opts into waiting.

## Why

Data can stay near users, survive the failure domains named on the cluster ladder, and use the right media (memory vs NVMe vs HDD) per datatype — without a separate clustering product and without treating geography as a voting set.

## Actors

- Cluster architect declaring the topology ladder, replication factor, and anti-affinity
- Tenant placing a datatype on NVMe vs HDD vs memory
- Replica set performing sync (source domain) or async (log-follower) replication under quorum (`12`)
- Query planner ranking replicas by ladder then RTT (`05`)

## Requirements

- Reserved near→far keys `rack`, `az`, `region`, `continent`, `planet`; cluster-declared ladder; nodes fill every ladder key.
- Hierarchy integrity across the ladder; default RF≥2 anti-affinity = finest ladder key; refuse unsatisfiable spread.
- Farness rank by first differing ladder key; RTT overrides; `LOCAL_*` is `quorum_domain`, not AZ.
- Ladder is not a `quorum_domain`. Write quorum arithmetic remains `12`.

## Out of scope for this feature

- Raft election mechanics for controllers (`06`)
- Migration strategies (`10`)
- Replication metric series (`08`)
- Cluster/node identity and join/leave (`11`) except join MUST present ladder labels
- Internode framing and HLC (`12`) except the leaderless/LWW rules stated here; RTT samples live on the internode fabric
- WAL/fsync meaning of ack (`13`)
- Query IR and shuffle (`05`) except the farness rank contract above

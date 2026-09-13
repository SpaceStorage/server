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

All options for data MUST be documented well with examples to start, but stay flexible enough to create complex cluster systems in many regions, continents, or even planets, grouping datatypes with data near the user and replicating slowly to remote regions.

### Node labels and anti-affinity

Each node can be labeled with different labels to ensure anti-affinity of replicas (by selected label). Example:

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

Shared datatypes can be replicated with a specified replication factor and shared by selected labels.

Label selection MUST be documented well with examples to start, and MUST stay flexible for planetary-scale grouping and slow remote replication.

### Drives and memory as labels

Each server can have different drives and types of drives: SSD, HDD, NVMe, etc. Disks MUST enrich node labels so various drives can be selected for different types of data.

In-memory storage: each node MUST specify explicitly memory size and labels for this type of storage. Different nodes can have different sizes of memory storage, or memory storage can be absent on some nodes.

### Quorum and who serves traffic

Storage and drivers MUST specify quorum level for a query like in Cassandra. Protocols that do not support this MUST add it via options or use defaults: write wait for two nodes (`ack == 2`), read from every node (`ack == 1`).

Every node can receive requests from the user and operate with data.

### Sync vs async replication

Replication can be synchronous or asynchronous.

- Synchronous replication MUST implement quorum level for **write** operations.
- Asynchronous replication MUST implement quorum level for **read** operations.
- All replication logic is based on **datatypes and data composition**.

## Why

Data can stay near users, survive rack/AZ/region failure according to labels, and use the right media (memory vs NVMe vs HDD) per datatype — without a separate clustering product.

## Actors

- Cluster architect defining labels, replication factor, and anti-affinity
- Tenant placing a datatype on NVMe vs HDD vs memory
- Replica set performing sync or async replication under quorum

## Out of scope for this feature

- Raft election mechanics for controllers (`06`)
- Migration strategies (`10`)
- Replication metric series (`08`)

---
speckit_command: specify
suggested_slug: type-system
source: server/start
read_after: 00-constitution.md
---

# Feature: Multiparadigm type system (L0–L4)

Specify the datatype levels that make SpaceStorage multiparadigm. The paradigm is the types stored, from simple foundation structures to high-level storage models and cross-level composition.

The list of primitives and models MAY be expanded in the future. Do not treat inventories as closed.

## L0 — Foundation primitives (types)

Data structures:

- Tuple
- Vector
- Linked List
- Stack / Queue / Deque
- Ring Buffer
- Hash Table
- B+tree
- Skip List
- Radix Tree / Patricia Trie
- Heap
- Bloom Filter
- KD-tree
- HNSW
- ScaNN
- Bitmap / Bitset

Storage primitives:

- MemTable
- SSTable
- WAL
- Append-only Segment

Storage layouts / engines:

- LSM Tree

Foundation primitives MUST support **memory, persistent storage, or hybrid storage** depending on their implementation.

Memory mode is a **volatile tier**: after a process restart on that node, container **definitions and options** MUST come back and **content MUST NOT**, unless replication (`04`) re-populates it. Persistent and hybrid content MUST come back from drives (`13`). Hybrid policy (what stays in memory vs on disk) is per type in the catalog.

Foundation primitives MUST be able to use different data encodings, compression algorithms, and encryption mechanisms.

Data encodings:

- Gorilla
- Delta encoding
- Dictionary encoding
- RLE

Compression:

- Snappy
- ZSTD
- LZ4

Encryption:

- AES-256-GCM (default)
- ChaCha20-Poly1305
- Different keys (by **reference**; authority and master key / KEKs are `14`)

## Value domains (scalars)

L0–L4 name **structures and models**. Writes also need **value domains**. The catalog MUST include at least:

- bool, int32, int64, uint64, float32, float64, decimal
- utf8, bytes, uuid
- timestamp (UTC, microsecond precision), date
- jsonb
- null

**Null** (typed empty), **missing field**, and **tombstone** (deleted, `13`) are three different states. Default collation is Unicode; a container MAY request binary comparison. Vector types MUST declare dimension and distance metric (`l2`, `cosine`, `inner_product`).

Schema-required vs schema-optional vs schema-free is per type in the catalog. Additive schema evolution applies in place; incompatible changes are transforms (`10`).

## L1 — Shared (distributed capabilities)

Specified in depth in `04-distribution-placement.md`. L1 applies to compatible foundation primitives and higher-level abstractions:

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

This feature MUST define L1 as a **capability layer over types**, not a separate database.

## L2 — Data abstractions

Based on shared or foundation types:

- Map
- Ordered Map
- Multimap
- Set
- Ordered Set
- Sequence
- Document abstraction
- Field Index
- Field Path
- Key/Value collection
- Bitmap index
- N-gram index
- Range index
- Spatial Index Primitive
- Vector collection
- Time-series segment
- Object
- Object Collection

## L3 — Storage models

Logical models for organizing, storing, and accessing data. Composed from lower-level primitives and abstractions; MAY also use foundation or distributed primitives directly when appropriate.

Structured storage:

- Relational Table
- Columnar Table
- Document Store

Indexes:

- Full-text Search
- Vector Search
- Spatial Search

Specialized stores:

- K/V Store
- Time Series
- Object Storage
- Log Stream

## L4 — Storage composition (cross-level)

Logically L4. MAY combine compatible primitives, abstractions, and storage models from different levels into logical storage objects.

Composition:

- Union
- Federated
- Materialized View

Distribution (as composition of placement, not a second L1):

- Distributed
- Partitioned
- Replicated
- Sharded

## Why

Users pick the type that matches the workload (table, document, vector, object, log, composition) instead of running several databases. Drivers and query execution see one abstract type interface.

## Actors

- Schema designer choosing L2/L3/L4 types inside a namespace
- Storage engineer configuring L0 layout, encoding, compression, encryption
- Query planner matching operations to the type’s capabilities

## Out of scope for this feature

- Label/disk/memory topology details (`04`)
- MapReduce engines (`05`)
- Metric names per datatype (`08`)
- WAL/fsync/ack contract, tombstone retention, backup (`13`)
- Key authority / KMS (`14`)

---
speckit_command: constitution
source: server/start
project: SpaceStorage
---

# Constitution input for SpaceStorage

Apply this file as the project constitution. It is **governance only**. Do not generate feature specs, code, or plans from this file. Deferred feature intents live in `01`–`16`. The **complete product** is this constitution plus `01`–`15`. The **first shippable binary** is `16-mvp-and-nongoals.md`; constitution MUSTs that `16` defers are still owed, not cancelled.

Project: **SpaceStorage** — a multiparadigm database. The paradigm is determined by the datatypes stored in the system. Storage has levels that start from simple types and end at high-level models.

## Core principles

### Rust-only stack

The database server MUST be written in Rust. All dependencies MUST be written in Rust. The administrative CLI MUST be written in Rust.

Rationale: one language for server, CLI, and dependencies keeps the monolith auditable and avoids mixed runtimes.

### Fully asynchronous Tokio runtime

All operations MUST run on the Tokio runtime with async/await. Database request work MUST be asynchronous and MUST NOT block worker threads.

Blocking durability operations (fsync and equivalent) MUST run off the async worker pool (a dedicated blocking pool). An acknowledgement that counts toward write quorum for persistent or hybrid data MUST wait for the durability contract in `13-durability-and-recovery.md`, not for an in-memory write alone.

Rationale: every protocol, storage path, and control-plane action shares one async executor; fsync is scheduled, not executed on a worker thread.

### Single-process multithreaded monolith

The server MUST be a monolith of one async process with many threads. Thread count MUST be configurable and MUST be based on the number of cores of the machine.

Rationale: one process owns local datatypes, protocols, and node state; scale-out is clustering, not in-process microservices.

### Type-driven multiparadigm

SpaceStorage is multiparadigm because of datatypes, not because of bolted-on engines. Foundation primitives (L0), shared/distributed capabilities (L1), data abstractions (L2), storage models (L3), and cross-level composition (L4) are the architecture. Lists of primitives, encodings, compression, encryption, and models MAY expand in the future. Foundation primitives MAY support memory, persistent, or hybrid storage depending on implementation.

Rationale: new capabilities appear as types and compositions, not as a second database product.

### Protocol compatibility on distinct ports

The **complete product** MUST implement PostgreSQL, Cassandra, Redis, Elasticsearch, ClickHouse, S3, and WebDAV protocols and APIs and work over them (compatibility **ceiling** in `15`; first binary in `16`). It MAY later implement its own protocol or add extra protocols. Each protocol SHOULD be implemented on a different port (strongly recommended). Drivers (for example PostgreSQL) MUST know about all datatypes and their features and MUST use all types via an abstract interface. "Unmodified **client**" (psql, redis-cli, …) on the documented MUST verb list is the promise; unmodified **applications** that need MUST-NOT verbs are out of scope.

Rationale: clients keep existing drivers; the type system is not hidden behind one protocol; shipping all seven handlers is not the first binary.

### Every node is a request coordinator

Every node MUST be able to receive requests from users and operate with data.

Rationale: no mandatory external proxy; any live node is a front door.

### Label-based planetary placement

All data options MUST be documented with examples to start, but MUST stay flexible enough to build clusters across many regions, continents, or planets as **label keys**, grouping datatypes with data near the user and replicating slowly to remote regions. **`planet` is a label, never an HLC or write-quorum domain.** The voting / HLC set is an explicit **`quorum_domain`** (`12`). Each cluster MUST declare an ordered **topology ladder** of reserved farness keys (`rack`, `az`, `region`, `continent`, `planet` — a subsequence for this cluster, not a product-wide fill-in). Member nodes MUST supply every key on **that** ladder (`04`). Replica anti-affinity MUST be selectable by label. Disks (SSD, HDD, NVMe, and others) MUST enrich node labels so different data types can select different drives. In-memory storage MUST be explicit per node (size and labels) and MAY be absent on some nodes.

Rationale: placement is a first-class product surface, not an afterthought of sharding.

### Cassandra-style quorum with protocol defaults

Storage and drivers MUST allow specifying quorum level for a query as in Cassandra. Protocols that do not support quorum MUST accept it via options or use global defaults: wait for write acknowledgement from two nodes (`ack == 2`) and read from every node with `ack == 1`. **Write `TWO` / `QUORUM` count only durable replicas in the container's source `quorum_domain` (`12`).** Replication MAY be synchronous (inside that domain) or asynchronous (log-followers outside it). Synchronous replication MUST implement quorum for writes. Asynchronous replication MUST implement quorum for reads. Replication logic is based on datatypes and data composition.

Rationale: one quorum model across protocols, with safe defaults when a wire protocol has no quorum field.

### Multi-tenant namespaces

The database is multi-tenant. Every tenant is a **namespace**. Each namespace has schema and data inside it, like a relational database. Each tenant MAY have different quotas and access policies for different types of data. Metrics and logs accumulate per tenant and globally.

Rationale: isolation, billing, and client self-service are namespace-scoped.

### Observability as a product surface

Primary monitoring systems are OpenTelemetry and Prometheus. Metric names MUST follow Prometheus naming conventions. Four Golden Signals is the main methodology. Each node calculates metrics only for its own datatypes; shared datatypes are aggregated by the primary server and stored in memory. Metrics are also used for billing and quota management. Buffer sizes MUST be configurable per node and buffer usage MUST be monitored. Server statistics are collected and stored in memory for each node. All types of data count access, hits/misses ratio, duration, and related stats.

Rationale: operators, tenants, and billing all consume the same metric model.

### Documented, expandable configuration

Labels, data options, and cluster topology MUST be documented with starter examples and MUST remain flexible for complex topologies. Primitive and model lists are not closed.

Rationale: a planetary cluster is configured from the same knobs as a laptop, with more labels.

### Raft controller elections and local restore

Different levels of controllers MUST implement elections using RAFT. Controllers own **metadata** (membership, namespaces, schemas, shared-datatype descriptors, role store) and **leadership requests**. The **data path is leaderless**: any replica may accept a write; the coordinator waits for quorum acknowledgements (`04`, `12`). A datatype that needs a single writer or a total order MUST request leadership from the control plane; it MUST NOT assume the datatype-level primary serializes every write.

When a node comes up it MUST restore **definitions and options** of all local datatypes. It MUST restore **content** of persistent and hybrid datatypes from drives (including WAL replay). Memory-mode content is a **volatile tier**: it MUST NOT be promised across process restart on that node unless replication re-populates it (`03`, `13`).

Rationale: control plane recovers without operator reconstruction; durable data returns from media; memory is a cache/volatile tier unless replicated.

### Security defaults for data and roles

Each node MAY encrypt data before storing it in drives or memory when specified for a data container. The role system manages the cluster. Roles MUST be stored in controller storage on the cluster level.

Rationale: encryption is per container; authority lives in the cluster controller, not on a random node.

## Extra governance sections

### Type-level architecture (closed for process, open for inventory)

The four-level stack (L0 foundation, L1 shared/distributed, L2 abstractions, L3 storage models, L4 composition) MUST be respected when adding features. New items MAY be added to the inventories. Features MUST NOT invent a parallel type hierarchy.

### Protocol and driver contract

A protocol adapter translates wire API to the abstract datatype interface. It MUST NOT become a second type system. Quorum, timeout, and namespace identity MUST be expressible even when the client protocol has no field for them (options or server defaults).

### Observability contract

Metric labels and series named in `08-observability.md` are constitution-adjacent product contracts. Plans MAY add series but MUST NOT rename or drop required labels without a constitution amendment.

## Governance

- **Amendments**: change this constitution via `/speckit.constitution` with an explicit rationale. Bump version: MAJOR for removed/redefined principles, MINOR for new principles, PATCH for wording.
- **Compliance**: `/speckit.plan` MUST include a constitution check. Violations need Complexity Tracking justification.
- **Intent split**: feature work goes through `/speckit.specify` using `01`–`16`. Do not fold those files into this constitution.
- **Source**: `server/start` is the original dump. Numbered intent files win for their domain after they exist. Gap-analysis files `11`–`16` win over `start` for identity, internode/clocks, durability, authz/keys, compatibility/limits, and MVP sequencing.

**Version**: 1.3.0 (proposed) | **Ratified**: 2026-09-13 | **Last Amended**: 2026-09-15

## Deferred non-governance intents (do not implement here)

Feed these to `/speckit.specify` using the sibling files, in order:

1. Runtime, CLI, TCP/HTTP interfaces — `01-runtime-cli-api.md`
2. Protocol adapters and drivers — `02-protocols-drivers.md`
3. L0–L4 type system — `03-type-system.md`
4. Distribution, labels, disks, quorum, replication — `04-distribution-placement.md`
5. Query execution and MapReduce — `05-query-execution.md`
6. Control-plane hierarchy, Raft, restore — `06-control-plane.md`
7. Tenancy, quotas, RBAC, encryption — `07-tenancy-security.md`
8. Metrics, logs, billing export — `08-observability.md`
9. Admin UIs and log ingest — `09-admin-ui-ingest.md`
10. Migration and transforms — `10-migration-transforms.md`
11. Cluster identity, discovery, join/leave/replace — `11-identity-membership.md`
12. Internode fabric, clocks, conflict resolution — `12-internode-and-time.md`
13. Durability, WAL, restore, deletes/TTL, backup — `13-durability-and-recovery.md`
14. AuthN, authZ vocabulary, master key / KEKs, audit — `14-authz-keys.md`
15. Protocol compatibility subsets, limits, rolling upgrade — `15-compatibility-and-limits.md`
16. MVP cut, sequencing, product non-goals — `16-mvp-and-nongoals.md`

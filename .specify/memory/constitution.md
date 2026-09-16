<!--
Sync Impact Report
- Version change: 1.2.0 → 1.3.0
- Modified principles: VII (cluster-declared topology ladder of reserved
  farness keys; member nodes MUST fill that ladder; planet remains a
  label, not a voting set)
- Added sections: none
- Removed sections: none
- Follow-up TODOs: re-specify 004 against the ladder; specify 11–16
-->

# SpaceStorage Constitution

## Core Principles

### I. Rust-Only Stack

The database server MUST be written in Rust. All dependencies MUST be
written in Rust. The administrative CLI MUST be written in Rust.

Rationale: one language for server, CLI, and dependencies keeps the
monolith auditable and avoids mixed runtimes.

### II. Fully Asynchronous Tokio Runtime

All operations MUST run on the Tokio runtime with async/await.
Database request work MUST be asynchronous and MUST NOT block
worker threads.

Blocking durability operations (fsync and equivalent) MUST run off
the async worker pool (a dedicated blocking pool). An acknowledgement
that counts toward write quorum for persistent or hybrid data MUST
wait for the durability contract in
`.specify/intent/13-durability-and-recovery.md`, not for an in-memory
write alone.

Rationale: every protocol, storage path, and control-plane action
shares one async executor; fsync is scheduled, not executed on a
worker thread.

### III. Single-Process Multithreaded Monolith

The server MUST be a monolith of one async process with many threads.
Thread count MUST be configurable and MUST be based on the number of
cores of the machine.

Rationale: one process owns local datatypes, protocols, and node state;
scale-out is clustering, not in-process microservices.

### IV. Type-Driven Multiparadigm

SpaceStorage is multiparadigm because of datatypes, not because of
bolted-on engines. Foundation primitives (L0), shared/distributed
capabilities (L1), data abstractions (L2), storage models (L3), and
cross-level composition (L4) are the architecture. Lists of primitives,
encodings, compression, encryption, and models MAY expand in the future.
Foundation primitives MAY support memory, persistent, or hybrid storage
depending on implementation.

Rationale: new capabilities appear as types and compositions, not as a
second database product.

### V. Protocol Compatibility on Distinct Ports

The **complete product** MUST implement PostgreSQL, Cassandra, Redis,
Elasticsearch, ClickHouse, S3, and WebDAV protocols and APIs and work
over them (compatibility ceiling in
`.specify/intent/15-compatibility-and-limits.md`; first binary in
`.specify/intent/16-mvp-and-nongoals.md`). It MAY later implement its
own protocol or add extra protocols. Each protocol SHOULD listen on a
different port. Drivers (for example PostgreSQL) MUST know about all
datatypes and their features and MUST use all types via an abstract
interface. "Unmodified **client**" (psql, redis-cli, …) on the
documented MUST verb list is the promise; unmodified **applications**
that need MUST-NOT verbs are out of scope.

Rationale: clients keep existing drivers; the type system is not hidden
behind one protocol; shipping all seven handlers is not the first
binary.

### VI. Every Node Is a Request Coordinator

Every node MUST be able to receive requests from users and operate with
data.

Rationale: no mandatory external proxy; any live node is a front door.

### VII. Label-Based Planetary Placement

All data options MUST be documented with examples to start, and MUST
stay flexible enough to build clusters across many regions, continents,
or planets as **label keys**, grouping datatypes with data near the user
and replicating slowly to remote regions. **`planet` is a label, never
an HLC or write-quorum domain.** The voting / HLC set is an explicit
**`quorum_domain`** (`.specify/intent/12-internode-and-time.md`). Each
cluster MUST declare an ordered **topology ladder** of reserved farness
keys (`rack`, `az`, `region`, `continent`, `planet` — a subsequence for
this cluster, not a product-wide fill-in). Member nodes MUST supply every
key on **that** ladder (`.specify/intent/04-distribution-placement.md`). Replica
anti-affinity MUST be selectable by label. Disks (SSD, HDD, NVMe, and
others) MUST enrich node labels so different data types can select
different drives. In-memory storage MUST be explicit per node (size and
labels) and MAY be absent on some nodes.

Rationale: placement is a first-class product surface, not an
afterthought of sharding.

### VIII. Cassandra-Style Quorum with Protocol Defaults

Storage and drivers MUST allow specifying quorum level for a query as in
Cassandra. Protocols that do not support quorum MUST accept it via
options or use global defaults: wait for write acknowledgement from two
nodes (`ack == 2`) and read with `ack == 1`. **Write `TWO` / `QUORUM`
count only durable replicas in the container's source `quorum_domain`
(`.specify/intent/12-internode-and-time.md`).** Replication MAY be
synchronous (inside that domain) or asynchronous (log-followers outside
it). Synchronous replication MUST implement quorum for writes.
Asynchronous replication MUST implement quorum for reads. Replication
logic MUST be based on datatypes and data composition.

Rationale: one quorum model across protocols, with safe defaults when a
wire protocol has no quorum field.

### IX. Multi-Tenant Namespaces

The database is multi-tenant. Every tenant is a namespace. Each
namespace MUST have schema and data inside it, like a relational
database. Each tenant MAY have different quotas and access policies for
different types of data. Metrics and logs MUST accumulate per tenant
and globally.

Rationale: isolation, billing, and client self-service are
namespace-scoped.

### X. Observability as a Product Surface

Primary monitoring systems are OpenTelemetry and Prometheus. Metric
names MUST follow Prometheus naming conventions. Four Golden Signals is
the main methodology. Each node MUST calculate metrics only for its own
datatypes; shared datatypes MUST be aggregated by the primary server and
stored in memory. Metrics MUST also be usable for billing and quota
management. Buffer sizes MUST be configurable per node and buffer usage
MUST be monitored. Server statistics MUST be collected and stored in
memory for each node. All types of data MUST count access, hits/misses
ratio, duration, and related stats.

Rationale: operators, tenants, and billing all consume the same metric
model.

### XI. Documented, Expandable Configuration

Labels, data options, and cluster topology MUST be documented with
starter examples and MUST remain flexible for complex topologies.
Primitive and model lists are not closed.

Rationale: a planetary cluster is configured from the same knobs as a
laptop, with more labels.

### XII. Raft Controller Elections and Local Restore

Different levels of controllers MUST implement elections using Raft.
Controllers own metadata (membership, namespaces, schemas,
shared-datatype descriptors, role store) and leadership requests.
The data path is leaderless: any replica may accept a write; the
coordinator waits for quorum acknowledgements. A datatype that needs
a single writer or a total order MUST request leadership from the
control plane; it MUST NOT assume the datatype-level primary
serializes every write.

When a node comes up it MUST restore definitions and options of all
local datatypes. It MUST restore content of persistent and hybrid
datatypes from drives (including WAL replay). Memory-mode content is
a volatile tier: it MUST NOT be promised across process restart on
that node unless replication re-populates it.

Rationale: control plane recovers without operator reconstruction;
durable data returns from media; memory is a cache/volatile tier
unless replicated.

### XIII. Security Defaults for Data and Roles

Each node MAY encrypt data before storing it in drives or memory when
specified for a data container. The role system MUST manage the cluster.
Roles MUST be stored in controller storage on the cluster level.

Rationale: encryption is per container; authority lives in the cluster
controller, not on a random node.

## Architectural Contracts

The four-level stack (L0 foundation, L1 shared/distributed, L2
abstractions, L3 storage models, L4 composition) MUST be respected when
adding features. New items MAY be added to the inventories. Features
MUST NOT invent a parallel type hierarchy.

A protocol adapter translates a wire API to the abstract datatype
interface. It MUST NOT become a second type system. Quorum, timeout, and
namespace identity MUST be expressible even when the client protocol has
no field for them (options or server defaults).

## Observability Contract

Metric labels and series named in `.specify/intent/08-observability.md`
are constitution-adjacent product contracts. Plans MAY add series but
MUST NOT rename or drop required labels without a constitution
amendment.

## Governance

This constitution supersedes conflicting local practice in specs, plans,
and code. `/speckit.plan` MUST include a constitution check. Violations
MUST be justified in Complexity Tracking or the change MUST NOT proceed.

Amendments MUST go through `/speckit.constitution` with an explicit
rationale. Versioning:

- MAJOR: backward-incompatible principle removal or redefinition
- MINOR: new principle or section, or materially expanded guidance
- PATCH: wording, clarifications, non-semantic refinements

`RATIFICATION_DATE` is the original adoption date and MUST NOT change.
`LAST_AMENDED_DATE` MUST be set to the amendment date. Reviews of specs
and PRs MUST verify compliance with these principles.

Feature implementation MUST NOT be folded into this document. Feature
work goes through `/speckit.specify` using `.specify/intent/01` through
`16`. `server/start` is the original dump; numbered intent files win for
their domain after they exist.

**Version**: 1.3.0 | **Ratified**: 2026-09-13 | **Last Amended**: 2026-09-15

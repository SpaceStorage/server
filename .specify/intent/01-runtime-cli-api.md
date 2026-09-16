---
speckit_command: specify
suggested_slug: runtime-cli-api
source: server/start
read_after: 00-constitution.md
---

# Feature: SpaceStorage runtime, CLI, and node interfaces

Specify the runtime and operator-facing control surfaces of the SpaceStorage database server.

## What

SpaceStorage is a multiparadigm database server. The server is a **monolith of one async process with many threads**. Thread count is configurable and based on the number of cores of the machine. All operations run on Tokio with async/await and are asynchronous and non-blocking. The server and its CLI are written in Rust; all dependencies are written in Rust.

The server MUST have its own **CLI and API as part of the database server** (not a separate product). Every administrator MUST enable **TCP and HTTP interfaces** for each database node.

Buffer sizes MUST be configurable for each node. Usage of each buffer MUST be monitored (metrics are specified in the observability intent; this feature owns the configuration knobs and the buffers themselves).

## Why

Operators need a single process they can size to the machine, turn on network admin/data interfaces per node, and tune buffers without a sidecar control binary.

## Actors

- Cluster administrator enabling interfaces and setting thread/buffer config
- Local operator using the Rust CLI on a node
- Database node process serving configured TCP and HTTP endpoints

## Requirements

- One async process per node; many worker threads; thread count configurable from core count (and overridable).
- Tokio runtime; no blocking operations on the runtime for database work.
- CLI in Rust, shipped with the server.
- Per-node TCP interface that an administrator must explicitly enable.
- Per-node HTTP interface that an administrator must explicitly enable.
- Per-node configurable buffer sizes.
- Every enabled entrypoint declares `tls { ... }` or `plaintext;`; omit is a startup error.
- `internode` and `replication` always listen (registered by `12`).

Drain: a stop signal MUST move the node to `draining` as specified in `01` runtime spec and `11` (no new tenant connections, no new replica placements). Blocking fsync MUST NOT run on worker threads (`00`, `13`).

Handlers `internode` and `replication` are registered by `12`; this feature owns the entrypoint model they plug into. Those two handlers **always listen**, even with no peers (`12`).

Every enabled entrypoint MUST declare **`tls { ... }`** or **`plaintext;`**. Omitted transport is a **startup error** (`14`). TLS is not globally mandatory.

## Out of scope for this feature

- Wire protocols for PostgreSQL/Cassandra/Redis/etc. (`02`)
- Type inventories (`03`)
- Cluster topology and Raft (`06`)
- Metric series catalog (`08`)
- Graphical admin UIs (`09`)

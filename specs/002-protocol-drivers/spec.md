# Feature Specification: Wire Protocols and Datatype-Aware Drivers

**Feature Branch**: `002-protocol-drivers`

**Created**: 2026-09-13

**Status**: Draft

**Input**: User description: "Read .specify/intent/02-protocols-drivers.md and specify this feature." — the intent file describes protocol compatibility for SpaceStorage: the server speaks PostgreSQL, Cassandra, Redis, Elasticsearch, ClickHouse, S3, and WebDAV so existing clients keep their drivers and tools; each protocol listens on its own port; every protocol driver sees the whole SpaceStorage type system through one abstract datatype interface and never hides types that exist in the store; every query carries a Cassandra-style quorum level and a timeout, supplied by the client where the protocol allows it and by administrator-set global defaults otherwise; every node accepts client requests; and each protocol's query language is executed by the shared query execution stack rather than a protocol-private engine.

## Clarifications

### Session 2026-09-13

- Q: What should happen when a query carries a quorum level that exceeds the container's replica count, given that the built-in write default is `TWO`? → A: Defaults clamp, explicit rejects. A quorum that came from a global or namespace default is clamped to the available replica count and reported as clamped; a quorum the client set explicitly per query or per session is rejected before execution if unsatisfiable.
- Q: How should a client session be bound to a namespace on the protocols that have no natural "select database" step (Redis, S3, WebDAV, Elasticsearch)? → A: Credential-bound. Every credential (principal) is bound to exactly one namespace; the protocol's own container concept (bucket, index, key, path) names a container inside that namespace; reaching another namespace requires different credentials. No namespace prefix is encoded in bucket, index, key, or path names.
- Q: Must a client be able to create a container of a type foreign to its protocol, or only read, write, and operate on existing containers? → A: Full lifecycle through every protocol. Each protocol's native create verb accepts a documented type option (default: the protocol's native type for that verb); any type in the inventory can be created, altered, and dropped from any protocol.
- Q: When a protocol has no native representation for a type, should all drivers use one shared canonical fallback representation or each its own protocol-idiomatic fallback? → A: One canonical fallback per type. The type system defines a single self-describing representation for each type; every driver carries it unaltered in its protocol's natural carrier (string, blob, JSON value, object body). Losslessness is a property of the type's canonical representation, not of each protocol pair.
- Q: Which ClickHouse wire transports must the complete product serve, and under what handler names? → A: Both transports as two handlers, each on its own entrypoint and both required in the **complete product**: `clickhouse` (native binary protocol) and `clickhouse-http` (HTTP interface). They are **not** required in the first shippable binary (`16`).

### Session 2026-09-15

- Q: Is the eight-handler matrix the first shippable binary? → A: No. The complete product MUST implement all eight handlers at the MUST/MUST NOT verb matrix in `15`. The first binary implements only PostgreSQL and Redis at the narrower subsets in `16`. Stock **clients** on documented MUST verbs are the promise; unmodified **applications** that need MUST-NOT verbs are out of scope.
- Q: What happens when a client issues a verb outside that protocol's MUST set? → A: The protocol's native not-supported error. Never a silent empty success. Exact lists live in `15`; this feature enforces the contract on the wire.
- Q: What does `LOCAL_*` mean for a protocol driver? → A: The coordinator's `quorum_domain` (`12`), not "same AZ" or "same region". This feature carries the token; arithmetic is `12`/`04`.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Connect with an existing client through its own protocol port (Priority: P1)

An application team already uses one of the supported databases or storage APIs. They point their unmodified client, driver, or command-line tool at a SpaceStorage node on the port the administrator declared for that protocol. The connection is accepted, the client's normal handshake and authentication complete, and the team runs their usual create, write, read, and delete operations against a namespace without changing application code. Each protocol lives on a separate port, so operators and network policy can tell the protocols apart, and a client that connects to the wrong port receives a clear refusal instead of a hang.

**Why this priority**: Protocol compatibility is the whole value proposition of this feature. Without stock clients connecting and working, nothing else here is demonstrable.

**Independent Test**: Can be fully tested by declaring one entrypoint per protocol handler on a single node (eight handlers: the seven protocols, with ClickHouse as two handlers), connecting a stock client for each handler, running a documented smoke workflow (create a container, write, read, delete) in each, and connecting each client to a port belonging to a different handler. Delivers a node usable from existing tooling with no other feature present beyond the runtime.

**Acceptance Scenarios**:

1. **Given** a node with entrypoints declared for handlers `postgresql`, `cassandra`, `redis`, `elasticsearch`, `clickhouse`, `clickhouse-http`, `s3`, and `webdav` on eight different ports, **When** the node starts, **Then** all eight listeners accept connections, the effective configuration lists each entrypoint with its handler, and no two protocol handlers share an address and port.
2. **Given** a running node and a stock PostgreSQL client, **When** the client connects to the `postgresql` entrypoint with valid credentials and a namespace name, **Then** the handshake completes, the client reports a ready session bound to that namespace, and a simple statement returns a well-formed result.
3. **Given** a running node and a stock Cassandra client, **When** the client connects to the `cassandra` entrypoint, **Then** the protocol negotiation completes, the client can list the namespace's schema, and a statement with an explicit consistency level executes.
4. **Given** a running node and a stock Redis client authenticating with a credential bound to namespace N, **When** the client issues a ping followed by a set and get, **Then** each command returns the protocol's expected reply, the stored value is returned unchanged, and the key lives in namespace N without any namespace prefix in the key name.
5. **Given** a running node and a stock Elasticsearch client, **When** the client calls the cluster information endpoint, creates an index, indexes a document, and searches it, **Then** each response is shaped as the protocol's clients expect and the document is found.
6. **Given** a running node and a stock ClickHouse native client, **When** the client connects to the `clickhouse` entrypoint and creates a table, inserts rows, and selects them, **Then** the rows are returned in the client's expected format.
6a. **Given** a running node and a stock ClickHouse HTTP-based client or tool, **When** it connects to the `clickhouse-http` entrypoint and runs the same table creation, insert, and select, **Then** the rows are returned in the HTTP interface's expected format and are the same rows the native client sees.
7. **Given** a running node and a stock S3 client, **When** the client creates a bucket, uploads an object (including one large enough to require a multipart upload), lists the bucket, downloads the object, and deletes it, **Then** every step succeeds and the downloaded bytes equal the uploaded bytes.
8. **Given** a running node and a stock WebDAV client, **When** the client lists a collection, uploads a file, reads it, moves it, and deletes it, **Then** every step succeeds and the file content round-trips unchanged.
9. **Given** a running node, **When** a PostgreSQL client connects to the `cassandra` entrypoint (or any client connects to a port declared for a different protocol), **Then** the handler rejects the connection within one second with a protocol-appropriate refusal, logs the mismatch with the entrypoint and handler, and the node keeps serving other connections.
10. **Given** a node whose configuration places two protocol handlers on the same address and port, **When** the node starts, **Then** startup fails and the error names both entrypoints and states that each protocol requires its own port.
11. **Given** a protocol entrypoint declared with transport encryption, **When** a stock client that supports the protocol's encrypted mode connects, **Then** the session is encrypted, and a plaintext attempt is refused as for any encrypted entrypoint.
12. **Given** two credentials bound to namespaces N1 and N2, **When** an S3 client creates bucket `data` with each credential in turn, **Then** two distinct containers exist (one per namespace), each session lists only its own `data` bucket, and neither bucket name carries a namespace prefix.
13. **Given** a credential with no namespace binding, **When** a Redis, S3, WebDAV, or Elasticsearch client authenticates with it, **Then** authentication is refused with the protocol's authentication error form and no session is created.

---

### User Story 2 - See and use the whole type system through any protocol (Priority: P1)

A developer using one protocol works with a container whose type is not native to that protocol — for example, a vector collection through Redis, a document store through PostgreSQL, or a time-series segment through the S3 API. The driver never pretends the type does not exist. The developer can list every container in the namespace with its SpaceStorage type, can read and write containers of every type through a documented mapping, and gets the same logical data back through a different protocol. Where the protocol has a native equivalent the driver uses it; where it does not, the driver uses a documented, lossless fallback representation and documented commands or paths for type-specific operations.

**Why this priority**: The constitution forbids a protocol adapter from becoming a second, narrower type system. A protocol that only exposes its native types would be exactly the "fake subset" the intent rules out. Ties with Story 1: compatibility without full type visibility fails the product's core promise.

**Independent Test**: Can be fully tested by creating one container of every type in the current L0–L3 inventory through each protocol in turn (using the type option on that protocol's native create verb), then listing, reading, writing, altering, and dropping each container through every other protocol, comparing the logical content. Delivers a type system reachable from every wire without depending on the query execution or distribution features beyond a single node.

**Acceptance Scenarios**:

1. **Given** a namespace containing one container of each type in the type inventory, **When** a client of any of the seven protocols asks the namespace what it contains (using that protocol's natural listing operation), **Then** every container appears with its SpaceStorage type name and no container is omitted because the protocol lacks a native equivalent.
2. **Given** a container whose type is native to the connecting protocol (for example a relational table through PostgreSQL, a key/value collection through Redis, an object collection through S3), **When** the client reads and writes it, **Then** values use the protocol's native representation and round-trip without loss.
3. **Given** a container whose type is not native to the connecting protocol (for example a vector collection through Redis), **When** the client reads it, **Then** the driver returns the type's canonical representation wrapped in the protocol's natural carrier (string, blob, JSON value, or object body), and the documentation states the canonical representation once per type and the carrier once per protocol.
4. **Given** a container of a non-native type, **When** the client writes a value in the type's canonical representation, **Then** the value is stored with full fidelity and is readable through a protocol where that type is native with identical logical content.
4a. **Given** a value of a non-native type read as a fallback through protocol X, **When** the same bytes are written back unchanged through protocol Y where the type is also non-native, **Then** the write is accepted and the stored value is identical to the original.
5. **Given** a value written through protocol X into a container of type T, **When** it is read through protocol Y, **Then** the logical value is the same, and for every ordered pair of protocols the round-trip is lossless for every type in the inventory.
6. **Given** a type with type-specific operations that the protocol has no native verb for (for example nearest-neighbour search over a vector collection through Redis or PostgreSQL), **When** the client invokes the documented mapping for that operation (a documented command, function, path, or query form), **Then** the operation executes against the shared execution layer and returns results in the protocol's result shape.
7. **Given** a client that attempts to write a protocol-native value the type cannot hold (for example a string into a bitmap index), **When** the write is submitted, **Then** the driver rejects it with the protocol's error form, names the container type and the offending value or field, and stores nothing.
8. **Given** a client of any protocol, **When** it issues the protocol's native create verb with the documented type option naming a type foreign to that protocol (for example creating a vector collection from a PostgreSQL session, or a relational table from an S3 session), **Then** a container of exactly that type is created in the namespace and is visible with that type through every protocol.
9. **Given** a client of any protocol, **When** it issues the protocol's native create verb without a type option, **Then** the container is created with that verb's documented default type (for example relational table for a SQL table creation, object collection for an S3 bucket, document store for an Elasticsearch index).
10. **Given** a container created from any protocol, **When** a client of any other protocol alters or drops it using the documented mapping, **Then** the change applies to the single shared container and is visible through every protocol.
11. **Given** a new type added to the type inventory in a later release, **When** the drivers are updated, **Then** every driver exposes the new type through the same abstract interface with a documented mapping; a driver that lacks a mapping for a type present in the store MUST be reported as incomplete by the node at startup rather than silently hiding the type.

---

### User Story 3 - Control quorum and timeout on every query (Priority: P2)

An application sets how many replicas must acknowledge a write and how many must answer a read, and how long a query may run, on each request — as Cassandra clients do. Through Cassandra the client uses the protocol's own consistency level and timeout. Through protocols that have no such fields, the client sets them as documented per-session or per-query options. When a client sets nothing, the administrator's global defaults apply: writes wait for two acknowledgements, reads wait for one, and the documented default timeout applies. Every query that reaches the execution layer carries both values, and the client can see which values were used.

**Why this priority**: One quorum and timeout model across protocols is a constitution principle and the distinction between SpaceStorage and a protocol shim. It depends on Stories 1 and 2 to have queries to attach options to.

**Independent Test**: Can be fully tested on a single node with a replica count of one and a test hook that records the options attached to each executed query: submit queries through each protocol with explicit, session-level, and absent options and verify the recorded quorum and timeout (with the default write quorum `TWO` recorded as clamped to `ONE` on a single replica, and an explicit `TWO` rejected). Delivers a verifiable option model independent of a live multi-node cluster.

**Acceptance Scenarios**:

1. **Given** a Cassandra client, **When** it executes a statement with its native consistency level set to `QUORUM` and a native request timeout, **Then** the executed query carries quorum `QUORUM` and that timeout, and no session or global default overrides them.
2. **Given** a client of a protocol without a native quorum field (PostgreSQL, Redis, Elasticsearch, ClickHouse, S3, WebDAV), **When** it sets quorum and timeout using that protocol's documented option mechanism for the session, **Then** every subsequent query in the session carries those values.
3. **Given** a client of a protocol without native fields, **When** it attaches quorum and timeout to a single query using the documented per-query mechanism, **Then** that query carries the per-query values and the next query in the session reverts to the session or global values.
4. **Given** a client that sets neither session nor per-query options, **When** it executes a write and then a read, **Then** the write carries the global write default (acknowledgement from two replicas, `TWO`), the read carries the global read default (acknowledgement from one replica, `ONE`), and both carry the global default timeout.
5. **Given** an administrator who changes the global defaults, **When** new queries arrive from clients that set no options, **Then** those queries carry the new defaults, and the effective configuration reports the current defaults.
6. **Given** a query whose timeout elapses before completion, **When** the timeout is reached, **Then** the client receives that protocol's timeout error form within one second of the deadline, the query stops consuming resources, and the error identifies the timeout that applied.
7. **Given** a client that explicitly requests (per query or per session) a quorum level the target data cannot satisfy (for example `THREE` when the container has two replicas), **When** the query is submitted, **Then** the driver returns the protocol's error form naming the requested level and the available replica count before any data is written.
8. **Given** a client that supplies an unparseable quorum value or a non-positive timeout, **When** the option is set, **Then** the driver rejects the option with the protocol's error form and the session keeps its previous values.
9. **Given** a client of any protocol, **When** it asks for the current session options using the documented inspection mechanism, **Then** the response shows the quorum and timeout in effect and whether each came from the query, the session, or the global default.
10. **Given** a client that sets no quorum options and a container with a single replica, **When** it executes a write, **Then** the global write default `TWO` is clamped to `ONE`, the write succeeds, and the session inspection mechanism shows the applied quorum as `ONE` with source "global default, clamped to replica count".

---

### User Story 4 - Configure protocol ports and global query defaults (Priority: P2)

An administrator decides which protocols a node speaks and on which ports by declaring one entrypoint per protocol handler, and sets the cluster-wide default quorum for writes and reads and the default query timeout. The node validates that every protocol has its own port, refuses to start on conflicts, and reports the resolved protocol map and defaults in its effective configuration and through the bundled CLI. A node may legitimately speak only a subset of protocols.

**Why this priority**: Operators need a predictable, validated way to expose protocols and set defaults before the platform is deployable, but the runtime feature already provides the entrypoint model this builds on.

**Independent Test**: Can be fully tested by starting nodes with all eight protocol handlers, a subset, an unknown protocol handler, duplicate ports, and various global default values, then reading the effective configuration through the admin surfaces. Delivers operator control of the protocol surface.

**Acceptance Scenarios**:

1. **Given** a configuration declaring entrypoints for only `postgresql` and `s3`, **When** the node starts, **Then** exactly those two protocol listeners open, the effective configuration shows the other protocol handlers as not declared on this node, and a client connecting to an undeclared protocol finds no listener.
2. **Given** a configuration that sets global write quorum `TWO`, global read quorum `ONE`, and a default timeout, **When** the node starts, **Then** the effective configuration reports those defaults with their provenance (configured or built-in default).
3. **Given** a configuration that sets a global default quorum to an unknown level, or a default timeout that is zero or negative, **When** the node starts, **Then** startup fails and the error names the setting and the accepted vocabulary or range.
4. **Given** a configuration that omits the global quorum and timeout defaults, **When** the node starts, **Then** the node applies the built-in defaults (write `TWO`, read `ONE`, documented default timeout) and reports them as built-in in the effective configuration.
5. **Given** a running node, **When** an administrator uses the bundled CLI or admin API to show the protocol map, **Then** the output lists each declared protocol handler, its address and port, its transport flag, its current connection count, and the global defaults.
6. **Given** a running node, **When** the administrator changes the global default quorum or timeout through a configuration reload, **Then** the new defaults apply to queries that start after the reload, in-flight queries keep the values they started with, and the effective configuration reflects the change without a restart.

---

### User Story 5 - Reach the same data from any node through any protocol (Priority: P3)

A client connects to any live node of the cluster on any protocol port and gets the same behaviour and the same data as on any other node. No node is a protocol-only gateway that lacks data, and no node is a data-only node that refuses clients. A write made through one node and one protocol is readable through another node and another protocol under the quorum semantics the client selected.

**Why this priority**: The constitution requires every node to be a request coordinator. This story is testable only with a multi-node cluster, so it follows the single-node stories.

**Independent Test**: Can be fully tested with a two-node cluster where each node declares the same protocol handlers: write through node A with protocol X, read through node B with protocol Y, and compare. Delivers protocol behaviour that is independent of the node chosen.

**Acceptance Scenarios**:

1. **Given** a two-node cluster where both nodes declare the eight protocol handlers, **When** a client connects to each protocol port on each node, **Then** every connection succeeds and the smoke workflow completes identically on both nodes.
2. **Given** a write with quorum `TWO` through node A over PostgreSQL into a container with two replicas, **When** the same container is read through node B over Elasticsearch with quorum `ONE` after the write is acknowledged, **Then** the written value is returned.
3. **Given** a cluster where node A declares a protocol handler that node B does not declare, **When** a client connects to that protocol on node B, **Then** it finds no listener, and the cluster's effective configuration shows the per-node protocol map so the operator can see the asymmetry.
4. **Given** a client connected to node A, **When** node A does not hold the data addressed by the query, **Then** the query is still answered by node A with the correct result and the client is not asked to reconnect elsewhere.

---

### User Story 6 - Native queries in every protocol run on the shared execution layer (Priority: P3)

A developer writes queries in each protocol's own language — SQL over PostgreSQL and ClickHouse, CQL over Cassandra, commands over Redis, query DSL over Elasticsearch, object and collection operations over S3 and WebDAV. Each driver hands the parsed request to the shared query execution stack rather than running a protocol-private engine, so the same dataset yields the same logical answer regardless of the language used, and every protocol benefits from the same planning, distribution, timeout, and quorum behaviour.

**Why this priority**: Native execution is what makes the type system usable rather than merely visible, but the parser, planner, and executor themselves are specified in the query execution feature. This story fixes the contract between drivers and that stack.

**Independent Test**: Can be fully tested by loading one dataset and running semantically equivalent queries in each protocol's language (filter, aggregate, range, nearest-neighbour where applicable), comparing result sets, and confirming through the execution layer's records that each query passed through the shared stack. Delivers a single execution path observable from every wire.

**Acceptance Scenarios**:

1. **Given** a dataset in a relational table, **When** an equivalent filter-and-aggregate query is run through PostgreSQL, ClickHouse, Cassandra, and Elasticsearch, **Then** the logical result sets are identical and each executed query is recorded as having passed through the shared execution layer with the protocol name attached.
2. **Given** a query in a protocol's language that uses a feature the shared execution layer does not yet support, **When** it is submitted, **Then** the driver returns the protocol's error form stating that the operation is not supported, naming the operation, and does not partially execute it.
3. **Given** a protocol operation with no query-language surface (for example an S3 object download or a WebDAV move), **When** it is submitted, **Then** it is executed through the same execution layer with the session's quorum and timeout attached and appears in execution records like any other query.
4. **Given** a query that touches containers of several types (for example a join between a relational table and a document store through PostgreSQL), **When** it is submitted, **Then** it is planned and executed by the shared layer and returns a result in the protocol's shape, or a clear "not supported" error naming the unsupported combination.

---

### Edge Cases

- Protocol whose native semantics span two wire transports (ClickHouse native binary and ClickHouse HTTP): each transport is a separate handler (`clickhouse`, `clickhouse-http`) on its own entrypoint; a single handler MUST NOT multiplex two transports on one port. A future protocol with several transports follows the same `<protocol>` / `<protocol>-<transport>` naming.
- Only one of the two ClickHouse handlers is declared on a node: allowed (FR-007); the effective configuration shows the other as not declared, and a native client connecting to the HTTP port (or vice versa) is refused as a protocol mismatch (FR-004).
- Client connects to the correct protocol port but speaks an unsupported protocol version: the driver refuses the negotiation with the protocol's version-mismatch form and lists the supported version range in the log; the node keeps serving.
- Client sends bytes that match no known handshake on a protocol port: the connection is closed after a documented idle or garbage limit; the event is counted per protocol and the node is unaffected.
- Client authenticates with credentials valid in the role system but not permitted for the namespace it selects: the connection is refused at namespace selection with the protocol's authorization error; no session is created.
- Protocol whose connection model has no namespace selection step (Redis, S3, WebDAV, Elasticsearch): the namespace is the one bound to the authenticated credential; a credential with no namespace binding is refused at authentication, and a bucket, index, key, or path is always interpreted inside the bound namespace (never as a namespace selector).
- Two namespaces each contain an S3 bucket (or Elasticsearch index) with the same name: no conflict, because a session sees only the containers of its credential's namespace; bucket and index names are unique per namespace, not cluster-wide.
- Two containers in the same namespace whose names collide only under a protocol's naming rules (for example case-insensitivity or forbidden characters in bucket names): the driver exposes both using a documented, reversible escaping and never silently merges or hides one.
- Protocol-native type with no SpaceStorage equivalent (for example a currency type in SQL): the driver either maps it to a documented SpaceStorage type with stated precision or rejects it at schema definition time with a clear error; it never stores a lossy conversion silently.
- Container type whose values exceed a protocol's single-message limits (for example a large object returned through Redis): the driver uses the protocol's documented chunking or streaming form, or rejects with a size error that names the limit; it never truncates.
- Quorum `LOCAL_QUORUM` explicitly requested when the coordinator's `quorum_domain` has no replicas of the container: rejected before execution with the requested level and the replica placement in the error; if `LOCAL_QUORUM` came from a default, it is clamped per FR-030 and the clamping is visible in the inspection mechanism. `LOCAL_*` is not "same AZ".
- Single-node or single-replica deployment using built-in defaults: writes proceed with the default `TWO` clamped to `ONE`; nothing needs to be configured for a first run, and the clamping is visible in the inspection mechanism and the execution record.
- Per-query option syntax collides with a construct of the protocol's own language (for example a SQL setting name that already exists in the protocol): the documented SpaceStorage option namespace takes precedence and the collision is documented.
- Session option change while a query is in flight in the same session: the in-flight query keeps its values; only subsequent queries see the new values.
- Timeout set by the client that exceeds an administrator-configured maximum: the driver clamps to the maximum, executes, and reports the clamped value in the session inspection mechanism.
- Node draining while protocol sessions are open: new connections on protocol ports are refused; in-flight queries complete within the drain window with their own timeouts still enforced; idle sessions are closed with the protocol's shutdown form.
- Elasticsearch, S3, or WebDAV client calls an endpoint the driver does not implement: the driver returns that protocol's not-implemented error form naming the endpoint; it never returns a success with empty data.
- Driver update introduces a native mapping for a type that previously used the canonical fallback: existing stored data remains readable; the native mapping applies to reads and writes going forward; the canonical representation remains accepted on write; the change is documented as a driver compatibility note.
- Canonical representation of a type changes in a later release: the type system versions the representation, drivers accept every documented version on write and emit the current version on read; no protocol-specific migration is needed because drivers do not alter the representation.

## Requirements *(mandatory)*

### Functional Requirements

**Protocol coverage and ports**

- **FR-001**: The **complete product** MUST implement client-facing protocol handlers for PostgreSQL, Cassandra, Redis, Elasticsearch, ClickHouse, S3, and WebDAV, registered in the node's handler inventory under the names `postgresql`, `cassandra`, `redis`, `elasticsearch`, `clickhouse` (ClickHouse native binary protocol), `clickhouse-http` (ClickHouse HTTP interface), `s3`, and `webdav` — eight handlers for seven protocols. Both ClickHouse handlers are required in the complete product; they share one driver behaviour (same type mappings, option handling, and namespace rule) and differ only in transport. The first shippable binary implements only the PostgreSQL and Redis subsets in `16`; a build that omits later handlers MUST still list them as not in this build when an entrypoint names them (`001`).
- **FR-002**: Each protocol handler MUST accept connections from unmodified stock clients of that protocol and complete the protocol's standard connection handshake, version negotiation, and authentication exchange.
- **FR-003**: Each protocol handler MUST be declared on its own entrypoint; two different protocol handlers MUST NOT share an address and port. A configuration that places two protocol handlers on the same address and port MUST fail startup naming both entrypoints. Any future exception to one-protocol-per-port MUST be a documented, explicitly configured choice and MUST be flagged in the effective configuration.
- **FR-004**: A protocol handler MUST reject a connection that does not speak its protocol within one second, using a protocol-appropriate refusal, MUST log the mismatch with entrypoint and handler, and MUST NOT affect other connections.
- **FR-005**: A protocol handler MUST refuse unsupported protocol versions with the protocol's version-mismatch form and MUST document the supported version range for each protocol.
- **FR-006**: The set of protocol handlers MUST be expandable: adding a protocol MUST consist of registering a new handler name in the inventory and providing a driver over the abstract datatype interface, without changing existing handlers.
- **FR-007**: A node MAY declare any subset of the protocol handlers; the effective configuration MUST list which protocol handlers are declared and which are not on that node.
- **FR-008**: Every protocol entrypoint MUST support the entrypoint transport-encryption declaration of the runtime feature; where the protocol has a native encrypted mode, the handler MUST use it so stock clients can connect encrypted.
- **FR-009**: Each protocol handler MUST connect a client to exactly one namespace per session. For protocols with a native selection step (database name for PostgreSQL and ClickHouse, keyspace for Cassandra) the client selects the namespace by that name and MUST be authorized for it. For protocols without a native selection step (Redis, S3, WebDAV, Elasticsearch) the namespace is the one bound to the authenticated credential; the protocol's own container concept (Redis key or key space, S3 bucket, WebDAV collection path, Elasticsearch index) names a container inside that namespace, and no namespace identifier is encoded in bucket, index, key, or path names. A request that cannot be mapped to a namespace, or that names a namespace the credential is not authorized for, MUST be rejected with the protocol's error form.
- **FR-009a**: Every credential usable on Redis, S3, WebDAV, or Elasticsearch MUST be bound to exactly one namespace in the role system; a credential without a namespace binding MUST be refused at authentication on those protocols with the protocol's authentication error form. Reaching a different namespace on those protocols requires a different credential.
- **FR-010**: Each protocol handler MUST authenticate clients using the protocol's native authentication exchange and MUST resolve the presented credentials against the cluster role system; the handler MUST NOT keep a protocol-private credential store. Authorization mechanics are specified in the tenancy and security feature.

**Abstract datatype interface and type visibility**

- **FR-011**: The server MUST provide one abstract datatype interface through which every protocol driver creates, lists, describes, reads, writes, deletes, and invokes type-specific operations on containers of every type in the type inventory (L0–L3 and L4 compositions).
- **FR-012**: Every protocol driver MUST operate exclusively through the abstract datatype interface; a driver MUST NOT define, store, or expose a type that is not in the shared type inventory.
- **FR-013**: Every protocol driver MUST expose every container in the connected namespace, with its SpaceStorage type name, through the protocol's natural listing or describe operation; no container MAY be omitted because the protocol lacks a native equivalent.
- **FR-014**: For every pair of (protocol, type in the inventory) the driver MUST have a documented type mapping that is either native (the protocol's own representation, used where a faithful equivalent exists) or fallback (the type's canonical representation carried in the protocol's natural carrier). The mapping MUST state the representation for reads, the accepted forms for writes, the documented commands, functions, paths, or query forms for the type's type-specific operations, and the create, alter, and drop forms for the type over that protocol.
- **FR-014a**: Every protocol's native create verb (for example a SQL table creation, a CQL table creation, an S3 bucket creation, an Elasticsearch index creation, a WebDAV collection creation, and a documented Redis creation command) MUST accept a documented type option naming any type in the inventory; when the option is absent the verb MUST create its documented default type. Any type MUST be creatable, alterable, and droppable from any protocol; a container is one shared object regardless of which protocol created it.
- **FR-015**: Every type mapping MUST be lossless: a value written through any protocol MUST be readable through any other protocol with identical logical content for every type in the inventory.
- **FR-015a**: The type system MUST define exactly one canonical, self-describing representation per type, used as the fallback by every driver. A driver MUST carry the canonical representation unaltered inside its protocol's natural carrier (string, blob, JSON value, or object body) on both reads and writes, MUST accept it on write for every type, and MUST NOT define a protocol-specific alternative fallback. Canonical bytes read through one protocol MUST be writable unchanged through any other protocol.
- **FR-016**: A driver MUST reject a write whose value cannot be held by the container's type, using the protocol's error form, naming the container type and the offending value or field, and MUST store nothing for that write.
- **FR-017**: A protocol-native type that has no faithful SpaceStorage equivalent MUST either be mapped to a documented SpaceStorage type with stated precision or rejected at schema definition time; silent lossy conversion is prohibited.
- **FR-018**: At startup a node MUST verify that every declared protocol driver has a mapping for every type in the node's type inventory and MUST report any missing mapping as a startup error naming the driver and the type, instead of hiding the type.
- **FR-019**: Container names that collide or are invalid under a protocol's naming rules MUST be exposed through a documented, reversible escaping; a driver MUST NOT silently merge, rename, or hide containers.
- **FR-020**: Values that exceed a protocol's single-message limits MUST be delivered through the protocol's documented chunking or streaming form, or rejected with an error naming the limit; truncation is prohibited.

**Quorum and timeout**

- **FR-021**: Every query that reaches the execution layer, through any protocol and including operations with no query-language surface (such as object download or collection move), MUST carry a quorum level and a timeout.
- **FR-022**: The quorum vocabulary MUST match Cassandra's consistency levels at minimum: `ONE`, `TWO`, `THREE`, `QUORUM`, `LOCAL_QUORUM`, `EACH_QUORUM`, `LOCAL_ONE`, `ALL`, plus an explicit numeric acknowledgement count; the vocabulary MUST be identical across all protocols.
- **FR-023**: Where a protocol has native fields for consistency level or request timeout (Cassandra), the driver MUST take the values from those fields.
- **FR-024**: Where a protocol has no native fields, the driver MUST accept quorum and timeout through a documented per-session option mechanism and a documented per-query option mechanism expressed in that protocol's own idiom (for example a session setting or query setting in SQL dialects, a command or command option for Redis, a request parameter or header for the HTTP-based protocols).
- **FR-025**: Option precedence MUST be per-query over per-session over namespace default (if the tenancy feature defines one) over global default; the value actually applied and its source MUST be inspectable by the client through a documented mechanism in each protocol.
- **FR-026**: The global defaults MUST be administrator-configurable and MUST default to write quorum `TWO` (acknowledgement from two replicas), read quorum `ONE` (acknowledgement from one replica), and a documented default timeout. Unknown quorum values or non-positive timeouts in configuration MUST fail startup naming the setting and the accepted vocabulary or range.
- **FR-027**: Global defaults MUST be live-reloadable; queries that start after the reload use the new values; in-flight queries keep the values they started with.
- **FR-028**: An administrator MAY set a maximum client timeout; a client timeout above it MUST be clamped and the clamped value reported through the inspection mechanism.
- **FR-029**: When a query's timeout elapses, the driver MUST return the protocol's timeout error form within one second of the deadline, the query MUST stop consuming resources, and the error MUST identify the timeout that applied.
- **FR-030**: A quorum level that the client set explicitly (per query or per session) and that cannot be satisfied by the addressed data's replica placement MUST be rejected before execution with an error naming the requested level and the available replicas. A quorum level that came from a global or namespace default and cannot be satisfied MUST instead be clamped to the highest satisfiable level for that data, the query MUST proceed, and the inspection mechanism (FR-025) and execution record (FR-037) MUST show the applied level with its source marked as clamped.
- **FR-031**: Invalid option values supplied by a client MUST be rejected in the protocol's error form and MUST leave the session's current options unchanged.

**Every node is a coordinator**

- **FR-032**: Every node MUST be able to serve every protocol handler it declares and to answer any query addressed to the cluster's data, whether or not the node holds that data locally; no node MAY be a protocol-only gateway without data, and no node MAY refuse client connections because it holds data.
- **FR-033**: Protocol behaviour, type mappings, option handling, and error forms MUST be identical on every node that declares the same handler.
- **FR-034**: The cluster's effective configuration MUST show the per-node protocol map so operators can see when nodes declare different protocol subsets.

**Native query path into the shared execution layer**

- **FR-035**: Each driver MUST parse requests expressed in its protocol's own language or operation set (SQL dialects, CQL, Redis commands, Elasticsearch query DSL and REST operations, S3 operations, WebDAV methods) and MUST submit them to the shared query execution stack specified in the query execution feature; a driver MUST NOT execute data operations through a protocol-private engine.
- **FR-036**: For semantically equivalent queries over the same data, drivers MUST produce identical logical results regardless of protocol.
- **FR-037**: Each executed query MUST be recorded by the execution layer with the protocol name attached, so that per-protocol accounting, metrics, and debugging are possible.
- **FR-038**: A request that uses a protocol feature or an operation combination the shared execution layer does not support, or a verb outside that protocol's complete-product MUST set (`15`), MUST be rejected with the protocol's not-supported error form naming the operation, without partial execution and without a silent empty success. First-binary PostgreSQL and Redis subsets are narrower (`16`); the same not-supported rule applies to verbs deferred from that binary.
- **FR-039**: Each driver MUST return results in the protocol's expected result shape, including for containers of non-native types (using the type mapping of FR-014).

**Session lifecycle and operability**

- **FR-040**: Protocol sessions MUST honour node draining: new connections are refused, in-flight queries complete within the drain window with their own timeouts enforced, and idle sessions are closed with the protocol's shutdown form.
- **FR-041**: Each protocol handler MUST report, through the admin surfaces and the bundled CLI, its declared entrypoints, current connection count, and supported protocol version range, alongside the global query defaults.
- **FR-042**: Each protocol handler MUST make available to the observability feature, labelled with the protocol name and namespace, at least: connection counts, request counts, error counts by class (protocol error, authentication, authorization, not supported, timeout, quorum unsatisfiable), request duration, and rejected-handshake counts.
- **FR-043**: Starter documentation MUST include, for every protocol: a complete example entrypoint declaration, the namespace mapping, the authentication exchange used, the session and per-query option syntax for quorum and timeout, the supported version range, and the type mapping table covering every type in the inventory.

### Key Entities

- **Protocol Handler (Driver)**: A named, client-facing behaviour declared on an entrypoint. Attributes: handler name (`postgresql`, `cassandra`, `redis`, `elasticsearch`, `clickhouse`, `clickhouse-http`, `s3`, `webdav`), protocol family (the two ClickHouse handlers share one), supported protocol version range, namespace mapping rule, authentication exchange, option mechanisms (session, per-query, inspection), type mapping table, error form catalogue, connection count. The inventory is expandable.
- **Abstract Datatype Interface**: The single contract through which every driver creates, lists, describes, reads, writes, deletes, and invokes type-specific operations on containers of every type. Owned by the type system; consumed by every driver.
- **Canonical Representation**: The single self-describing, versioned representation of a type defined by the type system and used as the fallback by every driver; carried unaltered in each protocol's natural carrier. One per type, not per protocol.
- **Type Mapping**: For one (protocol, type) pair: mapping kind (native or fallback), read representation (the protocol's native form, or the canonical representation plus the protocol's carrier), accepted write forms, documented surfaces for the type's type-specific operations, create/alter/drop forms (including the type option on the protocol's native create verb and whether this type is that verb's default), documented precision notes. Constraint: lossless across every protocol pair.
- **Client Session**: One authenticated connection over one protocol bound to exactly one namespace. Attributes: protocol, node, principal, namespace, namespace source (client-selected for PostgreSQL/ClickHouse/Cassandra; credential-bound for Redis/S3/WebDAV/Elasticsearch), session-level quorum and timeout, transport flag, start time, in-flight queries.
- **Credential Namespace Binding**: The role system's association of one credential with exactly one namespace, mandatory for credentials used on Redis, S3, WebDAV, and Elasticsearch; consulted at authentication to bind the session.
- **Query Options**: The quorum level and timeout attached to one executed query, each with its source (query, session, namespace default, global default) and any clamping applied (quorum clamped to replica count for default-sourced values; timeout clamped to the administrator maximum).
- **Quorum Level**: A value from the shared vocabulary `ONE`, `TWO`, `THREE`, `QUORUM`, `LOCAL_QUORUM`, `EACH_QUORUM`, `LOCAL_ONE`, `ALL`, or an explicit acknowledgement count; interpreted identically by every protocol.
- **Global Query Defaults**: Administrator-set write quorum (default `TWO`), read quorum (default `ONE`), default timeout, and optional maximum client timeout; live-reloadable; reported with provenance in the effective configuration.
- **Protocol Map**: The per-node list of declared protocol handlers with address, port, transport flag, and connection count; aggregated cluster-wide so operators can see asymmetries.
- **Type Catalog View**: The namespace's containers with their SpaceStorage type names as presented through a given protocol's listing operation, including reversible name escaping where required.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: For each of the eight protocol handlers (seven protocols, ClickHouse counted twice for its native and HTTP transports), an unmodified stock client completes the documented smoke workflow (connect, authenticate, create a container, write, read, delete) against a single node with zero client-side changes, in 100% of conformance runs.
- **SC-002**: 100% of types in the current type inventory appear with their type name in every protocol's listing operation, 100% of (protocol, type) pairs have a documented type mapping, and a container of every type can be created, altered, and dropped from every protocol in 100% of conformance runs.
- **SC-003**: A conformance suite that writes one value of every type through each protocol and reads it back through every other protocol reports zero lossy round-trips across all ordered protocol pairs, and canonical fallback bytes read through any protocol are accepted unchanged on write through every other protocol in 100% of cases.
- **SC-004**: 100% of queries in the conformance suite are recorded with a quorum level and a timeout; queries submitted without options carry exactly the global defaults (`TWO` for writes, `ONE` for reads, the documented timeout) or, where the replica count is lower, the clamped level marked as clamped; queries with explicit options carry exactly the supplied values, and 100% of explicit unsatisfiable levels are rejected before execution.
- **SC-005**: 99% of queries whose timeout elapses return a protocol-appropriate timeout error within one second of the deadline; 100% stop consuming resources within the same window.
- **SC-006**: Semantically equivalent queries run through different protocols over the same dataset return identical logical result sets in 100% of conformance cases, and 100% of those queries are recorded as passing through the shared execution layer with the protocol name attached.
- **SC-007**: In a two-node cluster, 100% of writes acknowledged at quorum `TWO` through one node and one protocol are readable through the other node and any other protocol at quorum `ONE` immediately after acknowledgement.
- **SC-008**: 100% of cross-protocol connection attempts (a client of protocol X connecting to a port declared for protocol Y) are refused within one second with no impact on other connections, and 100% of configurations placing two protocol handlers on one port are rejected before any listener opens.
- **SC-009**: A developer following the starter documentation connects an existing application using any one of the seven protocols and completes the smoke workflow within 15 minutes on first attempt.
- **SC-010**: Starter documentation contains one complete node configuration declaring all eight protocol entrypoints plus global query defaults, and that configuration passes CLI validation unchanged.
- **SC-011**: Per-protocol connection, request, error-class, and duration figures labelled with protocol name and namespace are available to the observability feature for 100% of declared protocol handlers.

## Assumptions

- Protocol names, handler names, and the quorum vocabulary in this spec are product requirements (per `.specify/intent/README.md` reading rules), not implementation choices. Language, runtime, and library mandates are left to the constitution and not repeated here.
- The entrypoint model (one port, one handler, mandatory `tls { ... }` or `plaintext;` declaration) is inherited from the runtime feature (`001`). This feature registers its eight handler names in that inventory and adds no second declaration form.
- "Different port per protocol" is treated as a MUST for this feature as the intent directs. A protocol family with more than one wire transport is modelled as one handler per transport, each on its own entrypoint. ClickHouse is served as `clickhouse` (native binary protocol) and `clickhouse-http` (HTTP interface), both required in the **complete product** (Clarifications, Session 2026-09-13 as amended 2026-09-15); they are not in the first shippable binary (`16`).
- Compatibility **ceiling** (wire versions and MUST/MUST NOT verbs) is intent `15`. The first binary's PostgreSQL/Redis subsets are intent `16`. Any endpoint or statement outside the applicable MUST set returns the protocol's not-supported form, never a silent empty success. Stock **clients** on MUST verbs are in scope; unmodified **applications** that need MUST-NOT verbs are not.
- Quorum semantics (what `QUORUM`, `LOCAL_*`, `EACH_QUORUM`, and acknowledgement counts mean) are defined by `12` (which replicas count; `LOCAL_*` is the coordinator's `quorum_domain`) and `04` (which nodes are replica targets). This feature carries the value on every query and validates satisfiability against placement before execution, with explicit unsatisfiable levels rejected and default-sourced levels clamped (confirmed in Clarifications, Session 2026-09-13).
- "Drivers MUST NOT hide types" is read as: every type in the inventory is creatable, alterable, droppable, listable, describable, readable, writable, and operable through every protocol, using native representation where one exists and the type's single canonical representation (defined once by the type system, carried unaltered by every driver) otherwise; creation of a foreign type uses a documented type option on the protocol's native create verb (both confirmed in Clarifications, Session 2026-09-13). The concrete canonical representation of each type is owned by the type system feature (`03`); the carrier per protocol, the syntax of the type option, and the syntax of type-specific operations over each protocol are planning decisions constrained by FR-014, FR-014a, and FR-015.
- Namespace identity per protocol: protocols with a native selection step use it — database name (PostgreSQL, ClickHouse) and keyspace (Cassandra). Protocols without one — Redis, S3, WebDAV, Elasticsearch — bind the session to the namespace of the authenticated credential, and buckets, indices, keys, and paths name containers inside that namespace (confirmed in Clarifications, Session 2026-09-13). A session addresses one namespace; cross-namespace queries are out of scope for this feature. The credential-to-namespace binding is stored and managed by the role system of the tenancy and security feature (`07`) with mechanisms in `14`.
- Timeout semantics inside the execution layer (partial results, cancellation propagation, asynchronous subscription to results) are defined by the query execution feature (`05`); this feature guarantees the value is carried and that the client receives the protocol's timeout error form.
- The default timeout value and the optional maximum client timeout are planning decisions; the default is on the order of tens of seconds, consistent with the defaults of the emulated systems.
- Authentication exchanges use each protocol's native mechanism; credential verification and authorization are provided by the cluster role system of the tenancy and security feature (`07`). Until that feature exists, the driver's authentication exchange is wired to a documented placeholder authority and the limitation is recorded in the plan.
- Global query defaults are cluster-wide settings; if the tenancy feature introduces per-namespace defaults they slot into the precedence chain of FR-025 between session and global.
- Metric series names, labels, and the scrape endpoint are owned by the observability feature (`08`); this feature guarantees the figures listed in FR-042 are tracked and labelled with protocol name and namespace.
- Ingest of logs over Kafka or syslog is a separate handler family owned by the admin UI and ingest feature (`09`) and is not a client protocol of this feature.
- Starter documentation, per-protocol type mapping tables, and a cross-protocol conformance suite are deliverables of this feature, consistent with the constitution's documented-configuration principle.

## Out of Scope

The intent file assigns the following to sibling features. This specification does not define them; it only reserves the touchpoints listed.

- **Type inventory and the L0–L4 stack** (intent `03`). This feature consumes the inventory through the abstract datatype interface and requires a mapping for every type in it; it does not define types.
- **Quorum semantics, replication modes, and placement** (intent `04`). This feature carries quorum on every query and validates satisfiability against placement; what a level means is defined there.
- **Parser, planner, scheduler, executor, MapReduce, and transaction internals** (intent `05`). This feature requires that every driver submits to that stack and receives its results and errors; the stack itself is specified there.
- **Cluster membership, controller elections, and the definition of a live node** (intent `06`).
- **Authorization rules, roles, quotas, and per-namespace policies** (intent `07`). This feature requires that drivers authenticate through the role system and bind sessions to one namespace.
- **Metric series catalogue and log sinks** (intent `08`).
- **Admin UIs and log ingest over Kafka or syslog** (intent `09`).
- **Cross-namespace queries in one session**, and **a SpaceStorage-native protocol**. Both are possible future additions through the expandable handler inventory (FR-006).
- **Complete-product verb matrix, wire versions, isolation, limits, N/N+1** (intent `15`) except enforcing not-supported on the wire.
- **First-binary handler cut** (intent `16`) except that a build may omit handlers not yet implemented and MUST fail startup if an entrypoint names a handler this build does not have (`001`).
- **Internode and replication ports** (intent `12`); they are not client protocols.
- **A second query engine per protocol** — product non-goal (`16`); every handler lowers into one shared query stack (`05`).
- **A SpaceStorage-native client protocol as a required slice** — product non-goal for slices 1–11 (`16`); constitution V MAY add a native protocol later, but there is no `handler spacestorage` in this inventory.

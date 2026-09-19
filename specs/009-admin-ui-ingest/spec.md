# Feature Specification: Cluster Admin UIs and Log Ingest

**Feature Branch**: `009-admin-ui-ingest`

**Created**: 2026-09-14

**Status**: Draft

**Input**: User description: "Read .specify/intent/09-admin-ui-ingest.md and specify this feature." — Cerebro-like topology UI (state, nodes, shards, data, replication progress); Kibana-like global and per-namespace configuration and data browsing across datatype levels; Kafka and syslog ingest into stored data (default L3 Log Stream); at-least-once Kafka with offset commit after durable ack; RFC 5424 preferred / 3164 accepted; UIs enforce `14` authz; served from the monolith; MVP may defer UIs (`16`) but this spec defines the complete product.

## Clarifications

### Session 2026-09-17

- Q: When someone who is not a cluster operator opens the Cerebro-like cluster map, what must they see? → A: `CLUSTER_ADMIN` / cluster `METRICS_READ` see the full map. `NAMESPACE_ADMIN` sees only their namespace. Anyone else is refused.
- Q: Does Kafka ingest listen on a port like syslog, or does SpaceStorage connect out to Kafka as a consumer? → A: Kafka ingest is a declared consumer attached to the ingest declaration. No Kafka listen port. Syslog remains a listen entrypoint.
- Q: How does an incoming syslog line get to the right namespace and container? → A: One dedicated `syslog` listen entrypoint per ingest declaration. That port writes only to the declared namespace and container.
- Q: Who is allowed to create or change an ingest declaration? → A: `NAMESPACE_ADMIN` (with `WRITE` on the target) may declare Kafka consumers in that namespace. Only `CLUSTER_ADMIN` may bind a `syslog` listen entrypoint. `WRITE` alone is refused.
- Q: How must a Kafka message be turned into a stored log record? → A: The ingest declaration names the payload format. Default is raw UTF-8 message body (key and broker timestamp as documented sidecar fields; invalid UTF-8 is a parse failure). JSON object mapping by field name is optional per declaration.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Cerebro-like cluster map (Priority: P1)

A cluster operator opens the Cerebro-like interface and sees database state, nodes, shards, data placement, and replication (and migration) progress. The view matches the topology and placement APIs of `04`/`06`/`11`, not a separate invented map. `CLUSTER_ADMIN` and cluster-scoped `METRICS_READ` see the full cluster map. `NAMESPACE_ADMIN` sees only that namespace's containers, shards, and replicas — never other tenants' shard maps. A principal with none of those rights is refused (`14`).

**Why this priority**: Operators cannot run a planetary cluster on CLI alone for day-two topology. First UI slice.

**Independent Test**: Three-node replicated container with induced lag; open the UI as cluster admin; confirm nodes/shards/lag; open as `NAMESPACE_ADMIN` and confirm only that tenant's map; open as a principal with no map rights and confirm refusal.

**Acceptance Scenarios**:

1. **Given** a three-node cluster with a replicated container, **When** a `CLUSTER_ADMIN` or cluster-scoped `METRICS_READ` principal opens the Cerebro-like UI, **Then** all member nodes, the container's shards/replicas, and replication progress are visible and match the cluster topology description.
2. **Given** a degraded replica, **When** the UI is viewed by a principal allowed to see that replica, **Then** that replica is shown degraded with lag or health matching `04`.
3. **Given** `NAMESPACE_ADMIN` on `acme` and no cluster topology rights, **When** they open the cluster map, **Then** they see only `acme` containers, shards, and replicas, and 0 other tenants' shard maps.
4. **Given** a principal with neither cluster map rights nor `NAMESPACE_ADMIN`, **When** they open the cluster map, **Then** they are refused.
5. **Given** a migration job (`10`), **When** it runs, **Then** progress is visible in this UI to principals who may see the affected containers (intent: progress MUST be trackable).

---

### User Story 2 - Kibana-like configure and browse (Priority: P1)

A cluster admin configures the cluster globally. A tenant admin configures their namespace. Both can browse data as tables and as other datatype levels (documents, KV, objects, …) using the query/execution stack (`05`), not a UI-private engine. All actions use `14` permissions.

**Why this priority**: Tenants need a console; constitution forbids a second type system in the UI.

**Independent Test**: Tenant creates a relational table and a document store through the UI, browses rows/documents, attempts a cluster-global setting, is refused.

**Acceptance Scenarios**:

1. **Given** `NAMESPACE_ADMIN` on `acme`, **When** they open the Kibana-like UI, **Then** they can configure `acme` settings they are allowed and browse `acme` containers as tables and other documented type views.
2. **Given** the same principal, **When** they attempt a cluster-global change, **Then** it is refused.
3. **Given** `CLUSTER_ADMIN`, **When** they use the global configuration view, **Then** they can change documented cluster settings that `01`/`06`/`07` expose.
4. **Given** a browse/query from the UI, **When** it runs, **Then** it is recorded as passing through the shared execution layer (`05`) with the UI as the application name for `08` labels when applicable.

---

### User Story 3 - Kafka and syslog ingest (Priority: P2)

Producers send logs to Kafka or syslog. SpaceStorage parses them into database data. The ingest declaration names namespace, container, and L3 type (default `Log Stream`). Kafka ingest is a **declared consumer** (brokers, topic, group) bound to that declaration — not a listen entrypoint. The declaration names the Kafka **payload format**; default is raw UTF-8 message body with key and broker timestamp as documented sidecar fields. JSON object mapping by field name is optional per declaration. It uses **at-least-once** delivery and commits offsets only after a durable acknowledgement (`13`). Syslog prefers RFC 5424 and accepts RFC 3164. Each syslog ingest declaration owns a dedicated `syslog` listen entrypoint (`01`); that port writes only to the declared namespace and container. `NAMESPACE_ADMIN` with `WRITE` on the target MAY declare Kafka consumers in that namespace. Only `CLUSTER_ADMIN` MAY bind a syslog listen entrypoint. `WRITE` alone is not enough. Exactly-once is not offered. Outbound logging remains `08`.

**Why this priority**: Intent's ingest path; distinct from metrics/log export.

**Independent Test**: Declare a log-stream container; send Kafka messages (default raw, and JSON when declared) and syslog lines; read them back through a protocol; kill a node mid-ingest and confirm at-least-once (possible duplicates, no silent loss of acknowledged messages).

**Acceptance Scenarios**:

1. **Given** ingest declared to namespace `acme` container `events` type `Log Stream` with default (raw) Kafka payload format, **When** UTF-8 Kafka messages are produced, **Then** they appear as data in `events` after durable ack with the value as the message body and key/broker timestamp as documented sidecar fields, and the consumer offset advances only then.
2. **Given** a syslog ingest declaration for `acme`/`events` bound to a dedicated `syslog` entrypoint, **When** RFC 5424 syslog is sent to that entrypoint, **Then** parsed records appear in `events` and not in any other container.
3. **Given** RFC 3164 syslog, **When** it is sent to that same dedicated entrypoint, **Then** it is accepted and stored with a documented parse.
4. **Given** ingest without a namespace or container declaration, **When** ingest would start, **Then** it refuses to run and names the missing target.
5. **Given** a crash after durable ack but before offset commit, **When** ingest resumes, **Then** duplicates MAY appear and previously acked messages are not lost.
6. **Given** a principal with `WRITE` on the target but not `NAMESPACE_ADMIN` or `CLUSTER_ADMIN`, **When** they try to declare Kafka or syslog ingest, **Then** configuration is refused (`14`).
7. **Given** two syslog ingest declarations, **When** they start, **Then** each has its own listen entrypoint; traffic to one port MUST NOT be stored in the other declaration's container.
8. **Given** `NAMESPACE_ADMIN` with `WRITE` on `acme`/`events`, **When** they declare a Kafka consumer for that container, **Then** the declaration is accepted; **When** they attempt to bind a `syslog` listen entrypoint, **Then** they are refused.
9. **Given** `CLUSTER_ADMIN`, **When** they bind a `syslog` entrypoint to `acme`/`events`, **Then** the declaration is accepted.
10. **Given** a Kafka ingest declaration with JSON payload format, **When** a JSON object value is consumed, **Then** named fields map onto the Log Stream record; **When** the value is not a JSON object, **Then** it is a parse failure (skip + error metric, partition not stalled).
11. **Given** default raw format, **When** a Kafka value is not valid UTF-8, **Then** it is a parse failure (skip + error metric, partition not stalled).

---

### Edge Cases

- UI deferred in an implementation milestone (`16`): this spec still applies to the complete product; the milestone MUST record the deferral, not delete the stories.
- Large object browse: respect `15` result size limits; paginate rather than hang.
- Kafka topic with mixed schemas under JSON format: records that fail parse are counted as errors (`08`) and MUST NOT block the partition indefinitely (skip or dead-letter per documented policy: skip with error metric is the default). Raw format treats only invalid UTF-8 as a parse failure.
- Two syslog declarations MUST NOT share a listen address and port (`01` unique entrypoints).
- Syslog burst filling buffers: reject/drop with metrics (`01` overflow), do not block worker threads.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The server MUST be accompanied by a Cerebro-like interface showing database state, nodes, shards, data, and replication (and trackable migration) progress.
- **FR-002**: The map MUST reflect topology/placement/membership from `04`/`06`/`11`, not a private model. `CLUSTER_ADMIN` and cluster-scoped `METRICS_READ` MUST see the full cluster map. `NAMESPACE_ADMIN` MUST see only that namespace's containers, shards, and replicas. Any other principal MUST be refused. The map MUST NOT show another tenant's shard map.
- **FR-003**: A Kibana-like interface MUST configure the cluster globally (admin) and per-namespace (tenant), and MUST show data in tables and other datatype-level views.
- **FR-004**: UI queries and configuration MUST use the shared execution and admin/control surfaces; the UI MUST NOT be a second query engine.
- **FR-005**: UIs MUST enforce the `14` permission vocabulary; they MUST NOT have a private privilege model. They are served from the monolith unless a later constitution amendment splits them.
- **FR-006**: The server MUST ingest Kafka messages into database data (ClickHouse-like). Ingest MUST declare namespace, container, L3 type (default `Log Stream`), and Kafka source (brokers, topic, consumer group). Kafka ingest MUST be a declared consumer that connects out to brokers. It MUST NOT be a listen entrypoint and MUST NOT open a Kafka listen port.
- **FR-007**: Kafka ingest MUST use a consumer group, at-least-once delivery, and commit offsets only after a counted durable acknowledgement (`13`/`04`).
- **FR-008**: The server MUST ingest syslog into database data. RFC 5424 is preferred; RFC 3164 is accepted. Each syslog ingest declaration MUST bind to its own `syslog` listen entrypoint (`01`). That entrypoint MUST write only to the declared namespace and container. Hostname, app-name, or facility MUST NOT be used as a product routing key across containers.
- **FR-009**: Exactly-once ingest MUST NOT be claimed in v1.
- **FR-010**: Parse failures MUST be counted and MUST follow the documented skip-or-dead-letter policy (default: skip with error metric).
- **FR-011**: Outbound logging to Kafka/syslog remains `08`; this feature MUST NOT confuse ingest with export.
- **FR-012**: Declaring a Kafka consumer MUST require `NAMESPACE_ADMIN` on the target namespace and `WRITE` on the target container, or `CLUSTER_ADMIN`. Binding or changing a `syslog` listen entrypoint MUST require `CLUSTER_ADMIN`. `WRITE` alone MUST be refused.
- **FR-013**: The Kafka ingest declaration MUST name a payload format. Default MUST be raw: the value is the message body as UTF-8; key and broker timestamp are stored as documented sidecar fields; a value that is not valid UTF-8 is a parse failure. A declaration MAY select JSON object mapping: JSON object keys map onto Log Stream fields by name; a value that is not a JSON object is a parse failure. Other formats are not required in this feature.

### Key Entities

- **Cluster Map View**: Nodes, shards, replica health, replication/migration progress. Full cluster for `CLUSTER_ADMIN` / cluster `METRICS_READ`; namespace-only for `NAMESPACE_ADMIN`.
- **Namespace Console**: Per-namespace config and data browser.
- **Ingest Declaration**: Namespace + container + type + source (Kafka consumer: brokers/topic/group + payload format, or syslog listen). Kafka payload format default is raw UTF-8 body; JSON mapping is optional. Kafka consumers: `NAMESPACE_ADMIN`+`WRITE` or `CLUSTER_ADMIN`. Syslog listen: `CLUSTER_ADMIN` only.
- **Kafka Consumer**: Outbound consumer bound to an ingest declaration; offset committed after durable ack. Not a listen entrypoint. Default payload is raw message body; JSON mapping optional.
- **Syslog Entrypoint**: `syslog` handler; one dedicated listen entrypoint per syslog ingest declaration; that port maps 1:1 to the declared namespace and container.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: An operator identifies a lagging replica from the Cerebro-like UI in under 2 minutes in 100% of lag fixtures in the suite.
- **SC-002**: A tenant with `NAMESPACE_ADMIN` completes create-container, insert, and table browse in the Kibana-like UI in under 15 minutes on first attempt following starter docs.
- **SC-003**: 100% of cluster-global actions attempted by a namespace-only principal are refused.
- **SC-004**: 100% of Kafka messages durably acked in the suite are readable from the target container; offset never advances past an unacked message. Default-raw UTF-8 samples store the value as the message body; invalid UTF-8 and non-object JSON (when JSON format is declared) increment parse errors and do not stall the partition.
- **SC-005**: RFC 5424 and RFC 3164 samples in the suite are stored; malformed lines increment errors and do not stall ingest.
- **SC-006**: 100% of UI data browses in the suite appear on the shared execution path (`05`).
- **SC-007**: 100% of syslog listen-entrypoint binds attempted by a non-`CLUSTER_ADMIN` principal are refused; 100% of Kafka ingest declarations attempted with `WRITE` and no `NAMESPACE_ADMIN`/`CLUSTER_ADMIN` are refused.

## Assumptions

- MVP (`16`) may ship without UIs; syslog may land as an `01` entrypoint and Kafka as declared consumers before the graphical chrome.
- Authentication of UI sessions uses `14` (token or same principal store).
- Kafka broker TLS is consumer configuration (certificate material referenced, never inlined), not an `01` listen entrypoint.
- Default skip-on-parse-error avoids poison-pill stalls; a dead-letter container MAY be added later.

## Out of Scope

- Metric series definitions (`08`).
- Query engine internals (`05`) except consuming them.
- Role vocabulary (`14`) except enforcing it.
- Replication streaming (`04`).
- Backup/PITR (`13`).
- **Kafka as a stored log product** — product non-goal (`16`); Kafka here is ingest into containers; Log Stream is the L3 type, not a Kafka log store.

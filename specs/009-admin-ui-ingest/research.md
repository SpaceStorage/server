# Research: Cluster Admin UIs and Log Ingest

**Feature**: `009-admin-ui-ingest` | **Date**: 2026-09-17

Each item resolves a Technical Context unknown or a technology choice. Format: Decision / Rationale / Alternatives considered. Inputs: [spec.md](spec.md) (clarify 2026-09-17), constitution 1.3.0, intent `09`, sibling plans `001` (entrypoint/`syslog` reserved, `ingest` block reserved, `admin-http`), `003` (`log_stream` append), `004`/`006`/`011` topology, `005` exec, `008` outbound Kafka/syslog (must not share the ingest path), `010` job progress, `014` permissions, `016` slice 11.

## R1. Two crates: `admin-ui` and `ingest`

- **Decision**: Add `crates/admin-ui` (`spacestorage-admin-ui`) for Cerebro-like and Kibana-like pages plus their HTTP routes, and `crates/ingest` (`spacestorage-ingest`) for the Kafka consumer, syslog handler, parsers, and ingest declarations. No new binary. `016` may ship ingest without chrome.
- **Rationale**: Spec stories are independently testable; `016` defers UIs and ingest together as slice 11 but a milestone MAY land handlers first. One crate would force UI assets into Kafka tests.
- **Alternatives considered**: Grow `node` (`001`) (rejected: ingest+UI dwarf runtime); one combined crate (rejected: deferral and test isolation).

## R2. Both UIs on `admin-http`, not new listen handlers

- **Decision**: Serve `/ui/cluster` (Cerebro-like) and `/ui/console` (Kibana-like) from the existing `admin-http` axum router. JSON for the map and console is under `/v1/cluster/map`, `/v1/console/*`, plus existing admin/control reads. No `handler cerebro` / `handler kibana` entrypoints. Optional later `handler ui;` (metrics-handler pattern from `008`) MAY expose only UI+read routes; not required for the complete product.
- **Rationale**: Clarify left serving unspecified; `001` already has HTTP/JSON + bearer. New handlers would add restart-required ports for chrome. Constitution III: monolith serves the UI.
- **Alternatives considered**: Separate UI entrypoints (rejected: extra ports, restart); standalone npm app (rejected: constitution I Cargo deps; split process).

## R3. HTML from Rust templates plus checked-in vanilla JS

- **Decision**: Pages are server-rendered with a pure-Rust template crate (`maud` or `askama`). Interaction (poll, table browse) is small vanilla JS **checked into the crate** (`assets/`), no `package.json`, no npm in CI. Charts are SVG/CSS. The JS files are assets, not Cargo dependencies.
- **Rationale**: Constitution I forbids non-Rust **dependencies**. A Cerebro/Kibana clone with npm would be a second stack. Templates keep the first paint on the server; JS only polls JSON.
- **Alternatives considered**: Leptos/Yew WASM (heavier, still legal); npm SPA (rejected: non-Rust toolchain); no JS / full reload (rejected: SC-001 map lag needs refresh without a full navigation).

## R4. Map refresh is HTTP poll, 2 s default

- **Decision**: The cluster map polls `GET /v1/cluster/map` every **2 seconds** (page may expose 1–10 s). No WebSocket, no SSE in this feature. `001` is HTTP/1.1 only.
- **Rationale**: SC-001 allows 2 minutes; 2 s is enough to see lag fixtures. WebSocket would be a new protocol on the admin port.
- **Alternatives considered**: WebSocket (rejected: HTTP/1.1 contract); operator-manual refresh only (rejected: day-two topology).

## R5. Kafka ingest is `rskafka` / `kafka-protocol`, never `rdkafka`

- **Decision**: Consume with a **pure-Rust** client (`rskafka` if consumer-group Fetch+OffsetCommit is available; otherwise Fetch/OffsetCommit via `kafka-protocol` in this crate). TLS `rustls`. **Not** `rdkafka`/`librdkafka`. One **consumer group** per Kafka ingest declaration; **all `ready` members** join the group (constitution VI). OffsetCommit runs **only after** a counted durable write ack (`013`/`004`).
- **Rationale**: Clarify Q2: outbound consumer, not a listen port. `008` R7 already forbade `rdkafka` for produce; ingest must match. Group protocol avoids every replica independently duplicating the whole topic (beyond at-least-once redelivery).
- **Alternatives considered**: Listen “Kafka handler” (rejected: Q2); `rdkafka` (rejected: C); one consumer per replica without a group (rejected: duplicate storm).

## R6. Syslog is UDP and TCP on the dedicated entrypoint

- **Decision**: Each syslog ingest is a `handler syslog` entrypoint (`001`). Transport is the entrypoint’s `tls` or `plaintext;` (TCP+TLS when `tls`). **UDP and TCP** both accepted on that same port when plaintext; TLS is TCP-only. RFC 5424 preferred: octet-counted framing on TCP, datagram or non-transparent on UDP. If 5424 parse fails, try RFC 3164. Else parse error (skip + metric).
- **Rationale**: Clarify Q3: 1:1 port → namespace/container. `001` already requires `tls` or `plaintext;`. 3164 is typically UDP; 5424 often TCP.
- **Alternatives considered**: TCP-only (rejected: 3164 senders); RFC 5425 as a third mode name (TLS is entrypoint `tls`, not a new handler).

## R7. Buffer overflow drops, never waits on a worker

- **Decision**: Register buffers `ingest.syslog.recv` and `ingest.kafka.decode`. Overflow policy **`reject`**: UDP datagram dropped; TCP line/frame not enqueued and the connection is closed after the in-flight decode; Kafka Fetch records that cannot enqueue are **not** OffsetCommitted (retry). Increment `spacestorage_ingest_dropped_total{reason="buffer_full"}`. Never `wait` on a Tokio worker.
- **Rationale**: Spec edge case “reject/drop”; constitution II. Waiting would stall the runtime. Not committing Kafka offsets on drop preserves at-least-once.
- **Alternatives considered**: `wait` policy (rejected: blocks workers); nack-and-commit Kafka on drop (rejected: silent loss of unacked messages).

## R8. Kafka declarations live in the cluster store; syslog bind is node config

- **Decision**: Kafka ingest objects are **cluster metadata** (`006` cluster log), visible on every node, live-applied (start/stop consumers on apply; not an `001` entrypoint restart). Syslog 1:1 bind is an **`ingest { namespace; container; type; }` child of the `syslog` entrypoint** in node config (restart-required, like all entrypoints). Admin HTTP/CLI for Kafka is the tenant path; syslog bind is editing node config (`CLUSTER_ADMIN`).
- **Rationale**: Clarify Q4: tenant admins attach Kafka; only cluster admins open listen ports. `001` entrypoints already restart; putting Kafka there would deny live tenant self-service.
- **Alternatives considered**: Both in node.conf (rejected: NAMESPACE_ADMIN cannot edit every node); both in cluster store including listen port (rejected: bind is per-node, `001` restart class).

## R9. Default Log Stream record

- **Decision**: Default Kafka **raw** format stores `{ timestamp, message, kafka_key, kafka_topic, kafka_partition, kafka_offset }` as documented sidecar fields. `timestamp` is the broker timestamp when present, else ingest time. `message` is UTF-8 value. JSON format maps object keys onto those names (and optional `severity`, `host`, `app_name`); unknown keys go in `attrs` (JSON object). Syslog fills `timestamp`, `message`, `severity`, `host`, `app_name`, `procid`, `msgid`, `facility` from the parsed header; structured data in `attrs`.
- **Rationale**: Clarify Q5; `003` Log Stream schema is optional. A closed column list keeps browse stable; `attrs` avoids dropping JSON fields.
- **Alternatives considered**: Opaque bytes only (rejected: no timestamp browse); JSON-only (rejected: Q5 default raw).

## R10. Writes use `log_stream` `append` through `005`

- **Decision**: Ingest does **not** write storage engines directly. It calls `append` (and UI browse calls `005` scan/query) with application name `spacestorage-ingest` or `spacestorage-ui` for `008` labels. Durable ack is the same counted quorum as any other persistent write (`004`/`013`).
- **Rationale**: Spec FR-004 / FR-007; constitution IV (no parallel engine).
- **Alternatives considered**: Direct WAL inject (rejected: skips quorum/types); UI-private query (rejected: FR-004).

## R11. Slice 11 vs first binary

- **Decision**: First binary (`016` slices 1–5): no UI routes (404), no `syslog` serve, no Kafka consumer. Handler **names** stay in the inventory as unregistered-or-stub like today. Config that enables `handler syslog`, an `ingest kafka` object, or `/ui/*` on a first-binary profile → `UiIngestSlice11Required`. Complete product / slice 11: this plan.
- **Rationale**: `016` MUST NOT require UIs or ingest in the first binary; complete product still owes them.
- **Alternatives considered**: Ship empty `/ui` in first binary (rejected: false product); compile ingest always (rejected: Kafka dep on slices 1–5).

## R12. UI authz is `014` on the same bearer as admin

- **Decision**: Until `014` lands, UI routes use the `001` admin token. After `014`: session is a principal; map/console enforce clarify Q1 (`CLUSTER_ADMIN` / cluster `METRICS_READ` full map; `NAMESPACE_ADMIN` namespace map; else 403). Console mutations use the same verbs as CLI (`NAMESPACE_ADMIN`, `CLUSTER_ADMIN`, `CREATE`/`WRITE`/…). No cookie-only privilege.
- **Rationale**: FR-005; intent `14`.
- **Alternatives considered**: Separate UI role names (rejected: spec); anonymous read-only map (rejected: Q1).

## R13. Cluster map is a filter over `004`/`006`/`010`/`011` views

- **Decision**: `GET /v1/cluster/map` composes topology (`004`), membership (`011`), replica health/lag (`004` replication), and migration job progress (`010`) when those crates exist. This crate **does not** invent node/shard identity. Tenant filter strips foreign-namespace containers and their shard rows; cluster node list for a tenant admin is **only nodes that host that tenant’s shards** (no other tenants’ container names).
- **Rationale**: FR-002; Q1 namespace-limited view.
- **Alternatives considered**: Private graph DB for the UI (rejected: FR-002); hide all node names from tenants (rejected: they need replica placement of their containers).

## R14. Metric names this feature adds

- **Decision**: `spacestorage_ingest_records_total{source,namespace,container,format,result}`, `spacestorage_ingest_parse_errors_total{source,namespace,container,reason}`, `spacestorage_ingest_dropped_total{source,reason}`, `spacestorage_ingest_kafka_offset_commits_total{namespace,container,result}`. `source` is `kafka` or `syslog`. `result` is `ok|parse_error|dropped`. Do not reuse `008` `spacestorage_log_export_*`.
- **Rationale**: Constitution X; `008` allows added series. Export vs ingest must not share counters (FR-011).
- **Alternatives considered**: Reuse export series (rejected: FR-011).

## R15. In-process Kafka for conformance

- **Decision**: Conformance uses an in-process or loopback **pure-Rust** fake broker (or `rskafka` test harness) plus a UDP/TCP syslog sender in `crates/conformance`. No Docker Kafka required for the suite.
- **Rationale**: Same as `006` in-process cluster; CI without C Kafka.
- **Alternatives considered**: External Kafka in CI (rejected: flaky, C stack).

No `NEEDS CLARIFICATION` remains in Technical Context.

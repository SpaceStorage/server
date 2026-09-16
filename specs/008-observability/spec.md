# Feature Specification: Observability, Billing Metrics, and Logging

**Feature Branch**: `008-observability`

**Created**: 2026-09-14

**Updated**: 2026-09-16

**Status**: Draft

**Input**: User description: "Read .specify/intent/08-observability.md and specify this feature." — Prometheus naming, OpenTelemetry and Prometheus as primary systems, Four Golden Signals; in-memory stats per node; per-datatype metrics on the owning node and aggregation of shared-datatype metrics by the primary; buffer monitoring; billing via metrics; per-namespace vs global metrics/logs; the full label and series catalog in the intent is a product contract.

## Clarifications

### Session 2026-09-16

- Q: Which events must SpaceStorage send on outbound logs (the global admin stream and any per-namespace Kafka or syslog sink)? → A: Default: lifecycle and failures only (node state changes, query/API/replication/job errors, job completion/failure, log-export failures). Successful queries are metrics-only unless a slow-query log is enabled. An audit/security log MAY be enabled (event content from `14`); both extra channels are off by default.
- Q: When a required metric label such as user or application is not known for a sample, what should the series do? → A: Omit the label key when the value is unknown. Presence means known. Never invent a value or use an empty token.
- Q: For the complete product, which ways must a tenant receive metrics for only their namespace? → A: Both a namespace scrape endpoint and OpenTelemetry push are implemented. Each namespace MAY enable scrape, OTel push, or both.
- Q: Which outbound log destinations must the complete product implement for the global admin stream and for a namespace private sink? → A: Both Kafka and syslog. A stream (global or per-namespace) MAY enable either or both. No other sink types are required.
- Q: When the control-plane primary is down, how must scrape output show that shared-datatype aggregated metrics are stale while local node metrics stay available? → A: Last aggregated values remain. A dedicated freshness series marks stale (0/1 plus last-success time). Local node metrics unchanged and not labelled stale.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Scrape global node and query metrics (Priority: P1)

A cluster administrator scrapes the primary `/metrics` endpoint (or receives OTel). Series follow Prometheus naming. Labels required by the intent are present on the corresponding families. Four Golden Signals (latency, traffic, errors, saturation) are covered by the query, API, communication, and system families. Each node emits metrics for its own datatypes; shared datatypes are aggregated by the primary (`06`) and held in memory. If that primary is down, last aggregated values remain scrapeable and a dedicated freshness series shows they are stale.

**Why this priority**: Observability is a constitution principle and the catalog is already a product contract.

**Independent Test**: Run a node, execute sample queries and replication, scrape `/metrics`, assert required names and labels exist; stop the aggregator primary and assert freshness series.

**Acceptance Scenarios**:

1. **Given** a ready node, **When** `/metrics` is scraped, **Then** system series include uptime, `node_ready`, `node_state`, worker thread counts/usage, and buffer usage (bytes and percent).
2. **Given** queries of several kinds, **When** metrics are scraped, **Then** query-processing series include totals, errors, in-flight, and histograms of execution, queue, and wait, labelled with kind, namespace, schema, datatype, and storage_type when those values are known.
3. **Given** a shared datatype on several nodes, **When** aggregated metrics are requested from the primary, **Then** they are present in memory there and not only as unaggregated local pieces, and the freshness gauge is 1 with a last-success timestamp.
4. **Given** buffer limit hits, **When** a buffer is filled, **Then** `buffer limit hits` (and usage) increase.
5. **Given** the aggregator primary is down, **When** a member node is scraped, **Then** local node series are present, last aggregated shared-datatype values remain, the freshness gauge is 0, the last-success timestamp is unchanged, and local series are not labelled stale.

---

### User Story 2 - Per-namespace metrics and logs (Priority: P1)

A tenant enables a namespace-only scrape endpoint, OpenTelemetry push, or both, and optionally a private Kafka and/or syslog sink. The tenant sees only their namespace. The administrator still has the global stream. Default outbound logs are lifecycle and failures only. A slow-query log and an audit/security log MAY be enabled independently (both off by default). Logging outbound is this feature; ingest of logs into tables is `09`; audit event content is `14`.

**Why this priority**: Multi-tenant isolation of telemetry is in `07` and constitution.

**Independent Test**: Two namespaces; enable scrape for one and OTel push for the other; compare tenant surfaces vs global; send logs to separate sinks.

**Acceptance Scenarios**:

1. **Given** `acme` private scrape enabled, **When** the tenant scrapes that endpoint, **Then** 0 series from other namespaces appear.
2. **Given** `acme` OTel push enabled, **When** the tenant collector receives a push, **Then** 0 series from other namespaces appear.
3. **Given** global `/metrics`, **When** an admin scrapes, **Then** series from all namespaces they may see appear (`14` METRICS_READ).
4. **Given** `acme` private logging with defaults to Kafka, syslog, or both, **When** operations occur, **Then** lifecycle and failure events for `acme` are delivered to each enabled sink, and successful queries do not appear as log lines.
5. **Given** a namespace without private export, **When** it operates, **Then** its metrics and default logs still appear in the global admin streams.
6. **Given** slow-query logging enabled with a duration threshold, **When** a query exceeds that threshold, **Then** a slow-query line is emitted on the configured sinks; queries under the threshold still do not log on success.
7. **Given** audit/security logging enabled, **When** a `14` audit event occurs, **Then** that entry is shipped on the configured sinks; with it disabled, outbound logs do not include the audit channel.
8. **Given** a namespace with neither scrape nor OTel push enabled, **When** it operates, **Then** no namespace-only metrics surface is exposed for it, and global admin streams still include its series.

---

### User Story 3 - Replication, API, durability, datatype, and background-job series (Priority: P2)

Operators diagnose replication lag, API traffic, WAL/fsync health, LSM/HNSW internals, and background jobs (compaction, TTL, backup, migration, …). Job **behavior** is owned elsewhere; this feature owns names. Traces/spans MAY be added later; metrics and logs are the required emit. No money formula for billing; series used for billing keep namespace and datatype labels.

**Why this priority**: The catalog is incomplete without these families; they are listed as MUST in intent.

**Independent Test**: Exercise replication, a compaction job, a WAL-backed write, an HNSW search; scrape and assert series.

**Acceptance Scenarios**:

1. **Given** async replication, **When** lag exists, **Then** replication series expose status, lag in seconds (and last block if possible), queues, retries, throughput, failures, with the replication labels.
2. **Given** protocol traffic, **When** scraped, **Then** user/application communication and API families expose totals, errors, connection gauges, and duration histograms with protocol, user, and application labels when those values are known (omitted when unknown).
3. **Given** durable writes, **When** scraped, **Then** durability series `db_wal_*`, `db_checkpoint_*`, `db_dirty_*`, `db_unflushed_bytes`, `db_recovery_*` exist as named in intent.
4. **Given** LSM and HNSW containers, **When** scraped, **Then** `db_lsm_*` and `db_hnsw_*` series named in intent exist.
5. **Given** a compaction (or other listed) job, **When** it runs, **Then** background-job series with job name and status exist; behavior correctness is `13`/`10`/`04`.
6. **Given** DNS lookups by the node, **When** they occur, **Then** `dns resolution` duration and request/error totals exist.

---

### Edge Cases

- Cardinality: high-cardinality labels (user, application) MUST be present when known. When unknown the label key MUST be omitted, never invented and never set to an empty token.
- Renaming or dropping a required label/series requires a constitution amendment.
- Primary down: local node metrics remain scrapeable and MUST NOT be labelled stale. Last shared-datatype aggregated values remain. Freshness MUST be a dedicated series (up/down gauge plus last-success timestamp), not a `stale` label on every aggregated series. Aggregated series MUST NOT be dropped solely because the primary is down.
- Billing consumer missing: metrics still emit; invoicing is out of this repository.
- Slow-query and audit/security log channels are off until enabled; enabling them MUST NOT change the default lifecycle-and-failure stream.
- Tenant sinks MUST NOT receive other namespaces' logs, including audit/security lines. Cluster-wide audit export MUST still respect `AUDIT_READ` (`14`).
- Key material MUST NEVER appear in outbound logs.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Metric names MUST follow Prometheus naming conventions. Primary systems are OpenTelemetry and Prometheus.
- **FR-002**: Methodology MUST cover the Four Golden Signals via the families in this spec.
- **FR-003**: Each node MUST calculate metrics only for its own datatypes; shared datatypes MUST be aggregated by the primary (`06`) and stored in memory. When that primary is down, last aggregated values MUST remain available (FR-024).
- **FR-004**: Server statistics MUST be collected and stored in memory per node. All types MUST count access, hits/misses where applicable, and duration.
- **FR-005**: Buffer usage MUST be monitored (bytes, percent, limit hits) (`01` supplies the figures).
- **FR-006**: Query-processing metrics MUST use labels: kind of query, namespace name, schema name, datatype name, node name if applicable, storage_type (`memory|nvme|ssd|hdd|etc.`), drive name or memory name, error type if applicable — and MUST expose: blocks reads/writes, bytes reads/writes, query totals and errors, rows/documents/values returned, hits and misses, lookup probes, index scanning and full scans, histograms of query execution / queue / waiting node, histogram of result size in bytes, retries total, query in flight. A listed label key MUST be omitted when its value is unknown.
- **FR-007**: User/application communication metrics MUST use labels: namespace, schema, datatype, node if applicable, user name, application name, protocol name, error type if applicable — and MUST expose: query totals and errors, communication status, histogram of `read()`/`write()` time, connection attempt counts and errors, actual connections gauge. A listed label key MUST be omitted when its value is unknown.
- **FR-008**: Replication metrics MUST use labels: namespace, schema, datatype, node if applicable, drive or memory name, protocol name, error type if applicable — and MUST expose: status, lag (seconds and last replicated block if possible), queues in blocks/bytes/objects, objects waiting, histogram of wait per block, retries, throughput, failures. A listed label key MUST be omitted when its value is unknown.
- **FR-009**: System metrics MUST use labels: namespace, schema, datatype, node if applicable — and MUST expose: uptime, buffer usage bytes and percent, cache hits/misses, cache eviction total, buffer limit hits, worker threads count, `db storage` read/write/fsync/flush `duration_seconds` and bytes, DNS resolution duration and requests/errors, worker thread usage percent and busy count, `node_ready`, `node_state` (`starting`, `ready`, `draining`, `degraded`, `recovering`, `failed`, etc.), leader elections total and errors, histogram of leader election duration. A listed label key MUST be omitted when its value is unknown.
- **FR-010**: API metrics MUST use labels: namespace, schema, datatype, node if applicable, user, application, protocol, error type if applicable — and MUST expose: connections, requests, request errors, histogram of request duration, active connections gauge. A listed label key MUST be omitted when its value is unknown.
- **FR-011**: Background-job metrics MUST use labels: job name, job status (`starting`, `running`, `completed`, `failed`, etc.), `job duration_seconds`, job errors total, job retries total, job queue size, `job queue duration_seconds`, job queue errors total — and MUST expose the queue totals listed in intent (completed/failed/starting/running totals and completed duration).
- **FR-012**: Background-job series MUST cover jobs: compaction, flush, checkpoint, vacuum, GC, index rebuild, replication, backup, cleanup, TTL expiration, snapshot, data transformation, data migration, data backup, data restore. Behavior owners: compaction/flush/checkpoint/vacuum/GC/TTL/snapshot/backup/restore → `13`; replication → `04`/`12`; index rebuild → `03`/`05`; transformation/migration → `10`; cleanup → the enqueueing feature.
- **FR-013**: Datatype-specific series MUST include `db_lsm_memtable_size_bytes`, `db_lsm_sstable_count`, `db_lsm_compaction_total`, `db_lsm_compaction_duration_seconds`, `db_lsm_compaction_pending`, `db_lsm_tombstones`, `db_hnsw_nodes`, `db_hnsw_edges`, `db_hnsw_search_duration_seconds`, `db_hnsw_search_ef`.
- **FR-014**: Durability series MUST include `db_wal_bytes_total`, `db_wal_fsync_total`, `db_wal_fsync_duration_seconds`, `db_checkpoint_total`, `db_checkpoint_duration_seconds`, `db_dirty_blocks`, `db_dirty_bytes`, `db_unflushed_bytes`, `db_wal_lag_bytes`, `db_wal_replay_duration_seconds`, `db_wal_recovery_duration_seconds`, `db_recovery_total`, `db_recovery_duration_seconds`, `db_recovery_records_total`.
- **FR-015**: Metrics MUST be usable for billing and quota management (namespace and datatype labels on usage series). This feature MUST NOT define a money formula.
- **FR-016**: The complete product MUST implement both a namespace-only scrape endpoint and OpenTelemetry push, each separate from global `/metrics`. Each namespace MAY enable scrape, OTel push, or both. Neither is required to be enabled for a given namespace. Global admin streams still include that namespace.
- **FR-017**: The complete product MUST implement outbound logging to Kafka and to syslog. Each stream (global admin or per-namespace) MAY enable Kafka, syslog, or both. No other sink types are required. A namespace MAY send logs to its own Kafka and/or syslog separately from the global admin stream.
- **FR-018**: Plans MAY add series but MUST NOT rename or drop required labels without a constitution amendment.
- **FR-019**: OpenTelemetry traces/spans MAY be added as series-adjacent product surface; the complete product MUST emit the metrics and logs specified here. Alerting/SLO documents are operator-side. Billing **money formula** is out of this repo; series used for billing MUST remain labelled with namespace and datatype.
- **FR-020**: Default outbound logs MUST include lifecycle and failures only: node state changes, query/API/replication/job errors, job completion/failure, and log-export failures. Successful queries MUST NOT produce a log line unless the slow-query channel is enabled and the query exceeds its threshold.
- **FR-021**: A slow-query log MAY be enabled (off by default). When enabled, queries whose duration exceeds a configured threshold MUST emit a log line on the configured global and/or namespace sinks.
- **FR-022**: An audit/security log MAY be enabled on outbound sinks (off by default). Event content is owned by `14`; this feature ships those entries when the channel is enabled. Tenant sinks MUST NOT receive other namespaces' audit. Cluster-wide audit export MUST still respect `AUDIT_READ` (`14`). Key material MUST NEVER appear in outbound logs.
- **FR-023**: When a required label's value is unknown, the series MUST omit that label key. Presence of the key means the value is known. Empty-string and sentinel values such as `unknown` MUST NOT be used as substitutes. Invented values are forbidden.
- **FR-024**: Shared-datatype aggregation freshness MUST be a dedicated series pair: an up/down gauge (1 = fresh, 0 = stale) and a last-success timestamp. When the aggregator primary (`06`) is down, last aggregated values MUST still be served; local node series MUST remain scrapeable and MUST NOT carry a stale label. Aggregated series MUST NOT be dropped solely because the primary is down. Exact Prometheus names for the freshness pair MAY be chosen in planning (FR-018).

### Key Entities

- **Metric Series**: Name + labels + type (counter, gauge, histogram) as catalogued above.
- **Global Metrics Endpoint**: Cluster-admin scrape/push surface.
- **Namespace Metrics Endpoint**: Tenant-only scrape or OTel push.
- **Log Sink**: Global or per-namespace Kafka and/or syslog destination.
- **Log Channel**: Default lifecycle-and-failure stream; optional slow-query channel; optional audit/security channel (`14` content).
- **In-Memory Stat Store**: Per-node and per-primary aggregation buffers.
- **Aggregation Freshness**: Dedicated up/down gauge plus last-success timestamp for shared-datatype aggregates.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of required label keys in FR-006–FR-011 appear on their families when the corresponding activity occurred and the label value is known; 0 of those series include an unknown label as an empty token or invented value.
- **SC-002**: 100% of named durability and LSM/HNSW series exist after exercising those components.
- **SC-003**: Tenant-only scrape and tenant OTel push each contain 0 foreign-namespace series in 100% of tests. A namespace with both enabled yields the same series set on scrape and push (label omission rules included).
- **SC-004**: Buffer usage scraped here agrees with admin CLI (`01`) to within one reporting interval.
- **SC-005**: An operator following starter docs scrapes `/metrics` and identifies node state, in-flight queries, and buffer saturation in under 10 minutes.
- **SC-006**: 100% of listed background-job types, when run, increment job series; 0 required series are renamed.
- **SC-007**: With default log channels, 100% of successful under-threshold queries produce 0 log lines; 100% of query/API/replication/job errors and node state changes in the suite produce a log line on the configured sink.
- **SC-008**: With slow-query logging disabled, 0 slow-query lines appear. With it enabled, 100% of queries over the threshold emit a line and 0 under-threshold successes do.
- **SC-009**: With audit/security logging disabled, 0 `14` audit events appear on outbound sinks. With it enabled, 100% of those events in the suite are shipped; 0 foreign-namespace audit lines appear on a tenant sink.
- **SC-010**: The product exposes Kafka and syslog sink types. 100% of tests that enable one, the other, or both deliver the same default-channel events to each enabled sink. 0 other sink types are required for conformance.
- **SC-011**: With the aggregator primary down, 100% of tests still scrape local node series; last aggregated shared-datatype values remain; freshness gauge is 0; last-success timestamp is unchanged; 0 local series carry a stale label. After the primary returns, 100% of tests show freshness gauge 1.

## Assumptions

- Figures are produced by the owning features (`01` buffers/threads, `05` queries, `04`/`12` replication, `13` WAL, `06` elections); this feature catalogues, stores in memory, and exports them.
- Exact Prometheus type (counter vs histogram buckets) is a planning decision constrained by the names above.
- Global export is Prometheus scrape at `/metrics` plus OpenTelemetry push. Per-namespace scrape and OTel push are the same two mechanisms, filtered to one namespace.
- Alerting rules and SLO documents are operator-side, not this feature.
- Slow-query duration threshold is a documented configuration knob; exact default milliseconds is a planning decision.
- Audit/security log *content* (which privileged actions, fields) remains `14`; this feature only optionally exports those entries to sinks.
- Outbound syslog encoding SHOULD match ingest preference: RFC 5424 (`09`). Extra sink products (beyond Kafka and syslog) are out of this feature.

## Out of Scope

- Cerebro/Kibana **display** (`09`).
- Who the primary aggregator is (`06`) except consuming that role.
- Job **behavior** (owners in FR-012).
- Billing invoices.
- Log ingest into tables (`09`).
- Defining the audit event vocabulary (`14`).
- Outbound sinks other than Kafka and syslog.

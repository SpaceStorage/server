# Data Model: Observability, Billing Metrics, and Logging

**Feature**: `008-observability` | **Date**: 2026-09-16 | **Spec**: [spec.md](spec.md)

Nothing in the **sample** path is Raft-persisted. Telemetry *configuration* for a namespace lives on the cluster log. Validation codes are in contracts.

## 1. MetricName

Prometheus metric identifier. `^[a-zA-Z_:][a-zA-Z0-9_:]*$`. Catalog names in [catalog.md](contracts/catalog.md). Intent `db_*` names are stored as-is (no `spacestorage_` prefix rewrite).

## 2. LabelSet

| Field | Type |
|-------|------|
| pairs | `BTreeMap<LabelName, LabelValue>` |

**Invariants**: A key is **absent** when the value is unknown (FR-023). Empty string and sentinels (`unknown`, `-`) are **forbidden**. Label names follow Prometheus `[a-zA-Z_][a-zA-Z0-9_]*`. Identity of a series = `(MetricName, LabelSet)`.

## 3. Sample (in-memory)

| Field | Type |
|-------|------|
| name | MetricName |
| labels | LabelSet |
| kind | `counter` \| `gauge` \| `histogram` |
| value | `AtomicU64` / `AtomicI64` for counter/gauge; histogram: `counts[bucket]`, `sum`, `count` |
| help | static catalog string |

Histogram buckets: [catalog.md](contracts/catalog.md) R10. Counters are monotonic in-process; restart resets (Prometheus convention).

## 4. Registry

| Field | Type |
|-------|------|
| series | map `(name, labels)` → Sample |
| interval | duration, default 1 s, for derived ratios (buffer percent) |

**Invariants**: One registry per process. Recorders only `inc`/`set`/`observe`. Encoder iterates under a short lock or lock-free snapshot. Overflow of a bounded log/OTLP queue increments export-error counters and a default-channel log line; it does not block query workers.

## 5. AggregationFreshness (in-memory, namespace voters)

| Field | Type |
|-------|------|
| namespace_id | UUID |
| datatype_id | UUID |
| namespace | current name (label) |
| datatype | current name (label) |
| up | `0` \| `1` |
| last_success | Unix timestamp seconds |
| merged | map of aggregated Samples for that datatype |

**Invariants**: `up=0` freezes `last_success` and `merged`. Local (unaggregated) series are not in this map and MUST NOT gain a `stale` label. Primary updates `merged` from `MetricsPush`; secondaries accept `MetricsSnapshot`. Lost if this process never received a snapshot.

## 6. LogChannel

| Value | Default | When |
|-------|---------|------|
| `default` | on | node state changes; query/API/replication/job **errors**; job completion/failure; log-export failures |
| `slow_query` | **off** | success or error with execution duration ≥ threshold |
| `audit` | **off** | `14` audit events this crate is asked to ship |

Successful under-threshold queries produce **no** log line.

## 7. LogEvent

| Field | Type |
|-------|------|
| time | RFC 3339 / RFC 5424 TIMESTAMP |
| channel | LogChannel |
| node | node name |
| namespace | optional; omitted on cluster-global lines |
| severity | `info` \| `notice` \| `err` |
| message | short text |
| fields | JSON object (query kind, duration, error type, …) |
| audit | optional `14` payload (principal, action, target, hlc) |

**Invariants**: No key material. Tenant sink: drop events whose `namespace` is missing or not this tenant. Cluster audit lines require global audit-export config (`AUDIT_READ` to enable).

## 8. SinkKind

`kafka` | `syslog`. No other kinds.

| Field | Kafka | Syslog |
|-------|-------|--------|
| destination | brokers + topic | host:port UDP or TCP |
| tls | rustls refs, never inline | optional rustls for TCP |
| enable | per stream | per stream |

A **stream** is `global` or `namespace { id }`. Each stream MAY enable Kafka, syslog, or both.

## 9. NamespaceTelemetry (cluster log, slice 9)

| Field | Type |
|-------|------|
| namespace_id | UUID |
| scrape | bool, default false |
| otel_endpoint | optional URL (OTLP/HTTP) |
| log_kafka | optional Kafka sink |
| log_syslog | optional syslog sink |
| slow_query | optional override of node default |
| audit_export | bool, default false (namespace-scoped events only) |

**Invariants**: `007` `private_metrics` / `private_logs` true on first-binary profile without this record → `ObservabilitySlice9Required`. Rename does not rewrite `namespace_id`; scrape path uses current name.

## 10. NodeMetricsConfig (file, `001` grammar)

See [config-directives.md](contracts/config-directives.md). Global OTel, global Kafka/syslog, slow-query default threshold (1 s, off), audit-export off.

## Relationships

```text
Process 1──1 Registry
Registry 1──* Sample
Namespace voter 1──* AggregationFreshness
ClusterState 1──* NamespaceTelemetry
Stream (global|ns) 0..1 Kafka sink
Stream (global|ns) 0..1 Syslog sink
LogEvent → 0..* enabled sinks on matching streams
```

## Validation codes

| Code | When |
|------|------|
| `ObservabilitySlice9Required` | tenant scrape/otel, Kafka/syslog, slow-query on, audit export on, on first-binary profile |
| `UnknownLabelForbidden` | recorder attempted empty/`unknown` label value (bug; drop sample + debug log) |
| `MetricsReadDenied` | scrape without `METRICS_READ` / admin token |
| `ForeignNamespace` | not used on scrape (filter silently); used if admin asks to export another tenant’s logs to this sink |
| `SinkConfigInvalid` | Kafka brokers empty, syslog address unparseable, OTLP URL not http(s) |
| `KeyMaterialForbidden` | log fields contained key bytes (`007`) |

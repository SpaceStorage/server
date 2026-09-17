# Phase 1 Data Model: Cluster Admin UIs and Log Ingest

**Feature**: `009-admin-ui-ingest` | **Date**: 2026-09-17 | **Plan**: [plan.md](plan.md) | **Spec entities**: [spec.md](spec.md)

Rust shapes are indicative. Normative HTTP/config surfaces are in [contracts/](contracts/).

---

## 1. `ClusterMapView` (spec: Cluster Map View) — FR-001, FR-002

Composed from `004` topology, `011` membership, `004` replica health/lag, `010` job progress. This crate does not store it.

| Field | Type | Notes |
|---|---|---|
| `generated_at` | HLC / wall clock | for poll freshness |
| `nodes[]` | from `004` topology | filtered for tenant (R13) |
| `containers[]` | `{ namespace, name, type, shards[] }` | shards/replicas + lag/health |
| `migrations[]` | `{ id, status, bytes_copied, bytes_remaining }` | omitted if `010` not present |
| `scope` | `cluster` \| `namespace` | `cluster` for `CLUSTER_ADMIN` / cluster `METRICS_READ` |

**Invariants**

- `scope=namespace` ⇒ 0 containers from other namespaces; node list only hosts of those containers (FR-002, clarify Q1).
- Principal with neither cluster map rights nor `NAMESPACE_ADMIN` never receives a body; HTTP 403 (US1.4).
- Shard identity equals placement description (`004`); UI MUST NOT mint replica ids.

---

## 2. `KafkaIngest` (spec: Ingest Declaration + Kafka Consumer) — FR-006, FR-007, FR-012, FR-013

Cluster-log body (`006`). Live-applied on every `ready` node.

| Field | Type | Notes |
|---|---|---|
| `id` | UUID | immutable |
| `name` | unique IDENT | renameable label |
| `namespace` | namespace name / id | target |
| `container` | container name | must exist |
| `type` | TypeName | default `log_stream` |
| `brokers[]` | host:port | TLS refs, never inlined |
| `topic` | string | |
| `group` | string | consumer group id |
| `format` | `raw` \| `json` | default `raw` |
| `transport` | `tls { cert, key }` \| `plaintext` | same rules as `001` entrypoints |
| `created_by` | principal id | audit `14` |

**Validation**

| Code | Rule |
|---|---|
| `ingest_missing_target` | namespace or container absent |
| `ingest_unknown_type` | type not in `003` catalog |
| `ingest_forbidden` | caller lacks `NAMESPACE_ADMIN`+`WRITE` on target and is not `CLUSTER_ADMIN` |
| `ingest_write_only` | `WRITE` without `NAMESPACE_ADMIN` / `CLUSTER_ADMIN` |
| `ingest_kafka_no_brokers` | empty brokers |
| `ingest_kafka_no_topic` | empty topic |
| `ingest_kafka_no_group` | empty group |
| `ingest_cert_inline` | PEM inlined |
| `UiIngestSlice11Required` | enabled on first-binary profile |

**State**: `starting` → `running` → `stopping` → `stopped`. Crash of a member: group rebalance; offsets only on Kafka after durable ack.

---

## 3. `SyslogIngestBind` (spec: Syslog Entrypoint) — FR-008, FR-012

Not a cluster object. Child of `entrypoint { handler syslog; ingest { … } }`.

| Field | Type | Notes |
|---|---|---|
| `entrypoint` | name | unique addr:port (`001`) |
| `namespace` | | required |
| `container` | | required |
| `type` | TypeName | default `log_stream` |

**Invariants**: one bind per syslog entrypoint; two binds MUST NOT share address:port; no hostname/facility routing key (FR-008). Mutating the bind is restart-required and `CLUSTER_ADMIN` only.

---

## 4. `LogRecord` (stored row / document) — FR-013, FR-010

Written with `log_stream.append`.

| Field | Raw Kafka | JSON Kafka | Syslog |
|---|---|---|---|
| `timestamp` | broker ts or ingest now | mapped or broker ts | parsed header |
| `message` | UTF-8 value | `message` key or stringify | MSG |
| `kafka_key` | key bytes as UTF-8 lossy | same | omitted |
| `kafka_topic` | topic | topic | omitted |
| `kafka_partition` | i32 | i32 | omitted |
| `kafka_offset` | i64 | i64 | omitted |
| `severity` | omitted unless JSON | optional | PRI severity |
| `host` | omitted unless JSON | optional | HOSTNAME |
| `app_name` | omitted unless JSON | optional | APP-NAME / tag |
| `procid` | omitted | optional | PROCID |
| `msgid` | omitted | optional | MSGID |
| `facility` | omitted | optional | PRI facility |
| `attrs` | empty | unknown JSON keys | RFC 5424 SD |

**Parse failure** (skip + metric, no OffsetCommit for that record, partition continues): raw value not UTF-8; JSON format and value not a JSON object; syslog line matches neither RFC.

---

## 5. `UiSession`

No extra store. Bearer principal from `001`/`014`. Application name for `008`: `spacestorage-ui`. Ingest application name: `spacestorage-ingest`.

---

## Relationships

```text
Principal --authz--> ClusterMapView (cluster | namespace | refuse)
NAMESPACE_ADMIN+WRITE | CLUSTER_ADMIN --create--> KafkaIngest
CLUSTER_ADMIN --bind--> SyslogIngestBind --1:1--> Entrypoint(handler=syslog)
KafkaIngest --consumer group--> brokers
KafkaIngest | SyslogIngestBind --append--> Container(log_stream) as LogRecord
LogRecord durable ack --then--> Kafka OffsetCommit
```

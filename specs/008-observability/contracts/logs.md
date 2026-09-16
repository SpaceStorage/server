# Contract: Outbound logs

**Feature**: `008-observability` | Spec: FR-017, FR-020–FR-022 | Slice: 9 for sinks; default channel still traces locally via `001` `log { }` in every binary

## Channels

| Channel | Default | Events |
|---------|---------|--------|
| `default` | on | `node_state` transitions; query/API/replication/job **errors**; job `completed`/`failed`; sink/export failures |
| `slow_query` | off | any query whose execution duration ≥ `threshold` (default 1s) |
| `audit` | off | `14` audit records this process is told to export |

Successful queries under threshold: **no line**. Enabling slow-query or audit MUST NOT disable `default`.

## Kafka (pure Rust)

JSON object per message:

```json
{
  "ts": "RFC3339",
  "channel": "default|slow_query|audit",
  "node": "...",
  "namespace": "acme",
  "severity": "info|notice|err",
  "message": "...",
  "fields": {},
  "audit": { "principal": "...", "action": "...", "target": "...", "time": "..." }
}
```

`namespace` omitted on cluster-global lines. Key: namespace or `_cluster`. TLS: rustls file refs. At-least-once.

## Syslog

RFC 5424. TCP octet-counted or UDP datagram. APP-NAME `spacestorage`. SD-ID `ss@32473` params: `ns`, `channel`, `node`. RFC 3164 outbound not required.

## Streams

- **Global**: node `log { kafka { … } syslog { … } }` — all namespaces the admin may see + cluster lines.
- **Namespace**: `NamespaceTelemetry` Kafka and/or syslog — that namespace only, including audit lines **for that namespace** when `audit_export` is on.

A stream MAY enable Kafka, syslog, or both. No other sink types.

## Isolation

Tenant sink MUST NOT receive other namespaces’ events. Cluster audit is not copied to a tenant sink. Enabling audit export globally requires `AUDIT_READ` (`14`). Key material → `KeyMaterialForbidden` (drop event).

## Local tracing

`001` `log { level; format; }` remains the process stderr/file log. Outbound sinks are additional. First binary: local tracing only; Kafka/syslog config → `ObservabilitySlice9Required`.

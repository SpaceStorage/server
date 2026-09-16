# Quickstart: scrape metrics, tenant isolation, freshness, sinks

**Feature**: `008-observability`

Proves SC-004, SC-005 on the first binary. SC-001–SC-003, SC-006–SC-011 need slice 9 (full catalog, tenant, logs, freshness). Prerequisites: [016 quickstart](../../016-mvp-and-nongoals/quickstart.md). Config: [metrics-block.conf](contracts/fixtures/metrics-block.conf). Catalog names: [catalog.md](contracts/catalog.md).

## 0. Config validation

```bash
spacestorage validate specs/008-observability/contracts/fixtures/invalid/otel-on-first-binary.conf
# first-binary profile expected: exit 2, ObservabilitySlice9Required

spacestorage validate specs/008-observability/contracts/fixtures/invalid/kafka-empty-brokers.conf
# expected: exit 2, SinkConfigInvalid
```

## 1. Global `/metrics` (SC-004, SC-005, first binary)

Start the one-node starter with `admin-http` enabled (`001`).

```bash
TOKEN=$(cat path/to/admin.token)
curl -sS -H "Authorization: Bearer $TOKEN" http://127.0.0.1:8080/metrics | head
# expected: HTTP 200 (not 404)
# expected: spacestorage_buffer_usage_bytes, spacestorage_buffer_usage_ratio,
#           spacestorage_buffer_limit_hits_total, node_ready, spacestorage_node_ready,
#           node_state, spacestorage_uptime_seconds, spacestorage_worker_threads
```

`spacestorage buffers` (CLI) percent and scrape `spacestorage_buffer_usage_ratio` agree within one reporting interval.

An operator following this section identifies `node_state`, in-flight queries (when `exec` records them), and buffer saturation in under 10 minutes.

## 2. Implemented-path labels (first binary)

Run a few PostgreSQL/Redis writes from the `016` smoke. Scrape again: query series MAY appear with `kind`, `namespace`, `datatype` when those crates record. Unknown `user` MUST NOT appear as `user=""` or `user="unknown"`.

## 3. Tenant scrape and OTel (slice 9; SC-003)

Complete-product profile. Two namespaces `acme` and `other`. Enable scrape for `acme` only.

```bash
curl -sS -H "Authorization: Bearer $TOKEN" \
  http://127.0.0.1:8080/metrics/namespaces/acme
# expected: 0 series with namespace="other"
# expected: 0 series without a namespace label

curl -sS -H "Authorization: Bearer $TOKEN" \
  http://127.0.0.1:8080/metrics/namespaces/other
# expected: 404 metrics_disabled
```

Enable OTel for `acme` and assert the collector’s payload matches the scrape series set (omit-label rules included).

## 4. Default logs vs slow-query vs audit (slice 9; SC-007–SC-010)

Enable global Kafka and/or syslog. With defaults: errors and node state changes appear; successful fast queries do not. Enable slow-query at `1ms` in the test; a >1ms query emits `channel=slow_query`. Disable audit export: 0 `14` audit payloads on the sink; enable: those events appear; `other` sink never sees `acme` audit.

## 5. Aggregator primary down (slice 9; SC-011)

Three-node cluster, shared datatype. Scrape the namespace primary: merged series + `spacestorage_shared_aggregation_up=1`. Stop that process. Scrape a secondary: last merged values still present, `up=0`, `last_success` unchanged, local series unlabeled. Scrape a remaining data node: local series present.

## 6. Out of scope here

Cerebro/Kibana (`09`), invoices, audit vocabulary (`14`), who is elected primary (`06`).

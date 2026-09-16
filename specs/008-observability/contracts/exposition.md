# Contract: Prometheus exposition

**Feature**: `008-observability` | Spec: FR-001, FR-016, FR-023, SC-001, SC-003 | Encoder: `crates/observability`

## Global scrape

- `GET /metrics` on `admin-http` (and on `handler metrics;` if declared).
- `Content-Type: text/plain; version=0.0.4; charset=utf-8` (OpenMetrics 1.0 MAY be offered via `Accept`).
- Auth: [admin-http-metrics.md](admin-http-metrics.md).
- Body: all series in the process registry, including local datatype series and, on namespace voters, merged shared-datatype series + freshness.

## Tenant scrape (slice 9)

- `GET /metrics/namespaces/{name}`
- Same content type.
- Include series **iff** label `namespace` equals `{name}` (current name after rename).
- 0 series from other namespaces. 0 node-global series (no `namespace` label).
- If scrape is not enabled for that namespace: **404** `metrics_disabled` (not an empty 200 that looks like “no traffic”).
- Unknown namespace: **404** `unknown_namespace`.

## Omit-label encoding

For each sample, write only keys present in `LabelSet`. Example legal pair:

```
spacestorage_query_total{kind="get",namespace="acme",schema="s",datatype="kv"} 4
spacestorage_query_total{kind="get",namespace="acme",schema="s",datatype="kv",user="alice"} 2
```

Illegal:

```
spacestorage_query_total{...,user=""} 1
spacestorage_query_total{...,user="unknown"} 1
```

`HELP`/`TYPE` lines once per metric name. Histogram `_bucket`/`_sum`/`_count` follow Prometheus rules; `le` is always present on buckets.

## Aliases

`node_ready` and `spacestorage_node_ready` MUST both appear with the same value (`001` + intent). Same for `node_state` / `spacestorage_node_state`.

## Errors

| HTTP | Code | When |
|------|------|------|
| 401 | `unauthorized` | missing/invalid auth |
| 403 | `MetricsReadDenied` | authenticated but no `METRICS_READ` |
| 404 | `unknown_op` | path not metrics (unchanged `001` on admin-http) |
| 404 | `metrics_disabled` / `unknown_namespace` | tenant path |
| 200 | — | global `/metrics` even if the catalog is sparse (first binary) |

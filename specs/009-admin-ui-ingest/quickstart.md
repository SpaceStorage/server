# Quickstart: cluster map, console, Kafka ingest, syslog ingest

**Feature**: `009-admin-ui-ingest`

Proves SC-001–SC-007 on **slice 11** (complete product). First binary: UIs and ingest are not required (`016`); enabling them must fail validation (`UiIngestSlice11Required`). Prerequisites: [016 quickstart](../../016-mvp-and-nongoals/quickstart.md) plus topology (`004`) and a `log_stream` container (`003`). Contracts: [ui-http.md](contracts/ui-http.md), [kafka-consumer.md](contracts/kafka-consumer.md), [syslog.md](contracts/syslog.md). Fixtures: [contracts/fixtures/](contracts/fixtures/).

## 0. First binary refuses ingest/UI config

```bash
spacestorage validate specs/009-admin-ui-ingest/contracts/fixtures/invalid/syslog-on-first-binary.conf
# first-binary profile expected: exit 2, UiIngestSlice11Required

spacestorage validate specs/009-admin-ui-ingest/contracts/fixtures/invalid/kafka-on-first-binary.conf
# expected: exit 2, UiIngestSlice11Required
```

## 1. Cluster map (SC-001, SC-003, clarify Q1)

Three-node cluster, replicated container, induced replica lag (`004`). Complete-product profile. `admin-http` enabled (`001`).

```bash
TOKEN=$(cat path/to/admin.token)   # CLUSTER_ADMIN / cluster METRICS_READ
curl -sS -H "Authorization: Bearer $TOKEN" http://127.0.0.1:8080/ui/cluster
# expected: 200 HTML

curl -sS -H "Authorization: Bearer $TOKEN" http://127.0.0.1:8080/v1/cluster/map
# expected: 200 JSON, scope=cluster, lagging replica visible, matches topology describe
```

As `NAMESPACE_ADMIN` on `acme` only: map `scope=namespace`, 0 other tenants' containers. As a principal with neither right: **403**. An operator following this section flags the lagging replica in under 2 minutes (SC-001).

Migration job (`010`) when present: `migrations[]` shows bytes copied / remaining.

## 2. Namespace console (SC-002, SC-003, SC-006)

```bash
# NAMESPACE_ADMIN on acme
# Create relational table + document store in the UI or:
# POST /v1/console/containers then insert and browse via POST /v1/console/query
```

Expected: first-attempt create → insert → table browse in under 15 minutes with starter docs. Cluster-global `POST /v1/console/config` as that tenant → **403** (100% in the suite, SC-003). Query traces as `application=spacestorage-ui` on `005`/`008` (SC-006).

## 3. Kafka ingest (SC-004, SC-007)

Create `acme.events` type `Log Stream`. Declare Kafka ingest (`format raw` default) as `NAMESPACE_ADMIN`+`WRITE`. Produce UTF-8 messages. Read them back via any protocol (`002`).

Expected: records durable; offset advances only after durable ack; sidecar topic/partition/offset present. Invalid UTF-8: parse error metric, partition not stalled. `WRITE`-only principal declaring ingest → **403** `ingest_write_only`.

Kill a node after durable ack and before OffsetCommit: duplicates MAY appear; acked messages still readable.

JSON format: object maps fields; a JSON array value is a parse error (skip).

## 4. Syslog ingest (SC-005, SC-007)

Starter: [ingest-syslog.conf](contracts/fixtures/ingest-syslog.conf). `CLUSTER_ADMIN` only. Send RFC 5424 and RFC 3164 to that port. Expected: stored in `acme.events` only. Second syslog entrypoint on another port → its traffic never appears in the first container. `NAMESPACE_ADMIN` bind attempt → **403**. Duplicate address:port → `entrypoint_duplicate_address`. Malformed lines increment errors and do not stall the listener.

## 5. Out of scope here

Metric catalog (`008` export), query engine internals (`005`), role vocabulary (`014` except enforcement), replica streaming (`004`), backup/PITR (`13`).

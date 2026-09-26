# Ingest / admin UI examples (009)

Starter fixtures (also under `specs/009-admin-ui-ingest/contracts/fixtures/`):

- `ingest-kafka.conf` — bootstrap Kafka consumer declaration
- `ingest-syslog.conf` — syslog entrypoint 1:1 to `acme/events`

## Map rights

| Principal | `/ui/cluster` and `/v1/cluster/map` |
|-----------|--------------------------------------|
| `CLUSTER_ADMIN` or cluster `METRICS_READ` | `scope=cluster` |
| `NAMESPACE_ADMIN` only | `scope=namespace` |
| other | 403 |

Default UI poll interval: **2 s** (`?interval=` 1–10).

## Authz for ingest

- Kafka declare: `NAMESPACE_ADMIN`+`WRITE` or `CLUSTER_ADMIN` (`WRITE` alone → `ingest_write_only`)
- Syslog bind: `CLUSTER_ADMIN` only (`ingest_syslog_cluster_only`)

First-binary profile rejects these configs with `UiIngestSlice11Required`.

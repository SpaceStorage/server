# Contract: Ingest declarations

**Feature**: `009-admin-ui-ingest` | Cluster object: `crates/ingest` + `006` | Node bind: `crates/config`

See [data-model.md](../data-model.md) for fields and error codes.

## Kafka (`KafkaIngest`)

Admin HTTP (also CLI in [cli.md](cli.md)):

| Method | Path | Authz |
|--------|------|-------|
| GET | `/v1/ingest/kafka` | list: `CLUSTER_ADMIN` all; `NAMESPACE_ADMIN` own namespace |
| POST | `/v1/ingest/kafka` | `NAMESPACE_ADMIN`+`WRITE` on target, or `CLUSTER_ADMIN` |
| DELETE | `/v1/ingest/kafka/{id}` | same as create |

`WRITE` without `NAMESPACE_ADMIN`/`CLUSTER_ADMIN` → `403 ingest_write_only` (FR-012, SC-007).

Missing namespace or container → `422 ingest_missing_target` (US3.4). Does not start a consumer.

Live-applied: cluster log commit → every `ready` node joins `group` (R5). Reload class: **live** (not an entrypoint).

## Syslog bind

Not a `/v1/ingest/syslog` create for tenants. Bind is node config (restart):

```text
entrypoint syslog-acme {
    address 0.0.0.0;
    port 5514;
    handler syslog;
    plaintext;   # or tls { certificate P; key P; }
    ingest {
        namespace acme;
        container events;
        type log_stream;   # default if omitted
    }
}
```

`CLUSTER_ADMIN` required to distribute/change this config. `NAMESPACE_ADMIN` attempting to add a syslog entrypoint via admin API → `403 ingest_syslog_cluster_only`.

Two syslog entrypoints with the same address:port → `001` `entrypoint_duplicate_address`. Omitting `ingest { namespace; container; }` → startup `ingest_missing_target` (handler refuses to run).

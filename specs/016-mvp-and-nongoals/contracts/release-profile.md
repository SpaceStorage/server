# Contract: Release profile (Cargo features and inventories)

**Feature**: `016-mvp-and-nongoals` | Crate: `crates/release-profile` | Spec: FR-002, Story 1 scenario 6

## Cargo features

Workspace root `Cargo.toml`:

```toml
[features]
default = ["first-binary"]
first-binary = [
  "handler-postgresql",
  "handler-redis",
]
complete-product = [
  "first-binary",
  "handler-cassandra",
  "handler-elasticsearch",
  "handler-clickhouse",
  "handler-s3",
  "handler-webdav",
]
handler-postgresql = ["dep:spacestorage-handler-postgresql"]
handler-redis = ["dep:spacestorage-handler-redis"]
handler-cassandra = ["dep:spacestorage-handler-cassandra"]
handler-elasticsearch = ["dep:spacestorage-handler-elasticsearch"]
handler-clickhouse = ["dep:spacestorage-handler-clickhouse"]
handler-s3 = ["dep:spacestorage-handler-s3"]
handler-webdav = ["dep:spacestorage-handler-webdav"]
```

`spacestoraged` always links `admin`, `admin-http`, `internode`, `replication` (not optional). ClickHouse crate registers **two** handlers (`clickhouse`, `clickhouse-http`) when the feature is on (`002`/`015`).

## Handler build set

`ReleaseProfile::handler_set()`:

| Handler | FirstBinary | CompleteProduct |
|---------|:-----------:|:---------------:|
| `admin` | required | required |
| `admin-http` | required | required |
| `internode` | required | required |
| `replication` | required | required |
| `postgresql` | required | required |
| `redis` | required | required |
| `cassandra` | **forbidden** | required |
| `elasticsearch` | forbidden | required |
| `clickhouse` | forbidden | required |
| `clickhouse-http` | forbidden | required |
| `s3` | forbidden | required |
| `webdav` | forbidden | required |
| `syslog` | not required | not required until slice 11 |

If a config names a forbidden or unlinked handler: `entrypoint_unknown_handler` listing `known` = registered names. No silent ignore.

## Type requirement (first binary)

Required **creatable** L3 names: `K/V Store`, `Relational Table`, `Document Store`. `Document Store` is admin-created; first-binary I/O is canonical blob over PostgreSQL and/or Redis (`002`). Other catalog entries MAY exist (`003`). First-binary conformance smokes all three.

## Profile detection at runtime

`spacestorage status --output json` (and admin `GET /v1/status`) includes:

```json
{
  "release_profile": "first-binary",
  "handlers": ["admin", "admin-http", "internode", "postgresql", "redis", "replication"],
  "slices_implemented_hint": "see docs/milestones"
}
```

`release_profile` is compile-time (`cfg`), not an operator setting. There is no config knob that “enables Cassandra” without rebuilding with `complete-product`.

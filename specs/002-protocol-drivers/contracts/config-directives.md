# Contract: Configuration Directives Added by This Feature

**Feature**: `002-protocol-drivers` | Extends [`001` config grammar](../../001-runtime-cli-api/contracts/config-grammar.md) | Implemented in `crates/config` (feature-registered blocks)

## New handler names (for `entrypoint.handler`)

`postgresql`, `cassandra`, `redis`, `elasticsearch`, `clickhouse`, `clickhouse-http`, `s3`, `webdav` — all `ClientFacing`, owner `02-protocol-drivers`. The `001` note "`cassandra` fails validation until `02` lands" is retired; `fixtures/node.conf` of `001` now validates with the production inventory.

Recommended (documented, never defaulted) ports: `postgresql` 5432, `cassandra` 9042, `redis` 6379, `elasticsearch` 9200, `clickhouse` 9000, `clickhouse-http` 8123, `s3` 9010, `webdav` 9020.

## New blocks

| Path | Arity | Type | Default | Reload | Notes |
|------|-------|------|---------|--------|-------|
| `query_defaults { write_quorum Q; }` | 1 | quorum literal | `TWO` | **live** | see [query-options.md](query-options.md) |
| `query_defaults { read_quorum Q; }` | 1 | quorum literal | `ONE` | **live** | |
| `query_defaults { timeout D; }` | 1 | DURATION 1ms–24h | `30s` | **live** | |
| `query_defaults { max_timeout D; }` | 1 | DURATION ≥ timeout | `10m` | **live** | |
| `auth { users_file P; }` | 1 | PATH | — | live (re-read) | Required when any client-facing handler is declared. Interim until `07`. |
| `protocols { <handler> { … } }` | block per handler | | | restart | Optional per-handler knobs (below) |

### Per-handler knobs (`protocols { … }`)

| Path | Type | Default | Notes |
|------|------|---------|-------|
| `protocols.postgresql.server_version` | STRING | `"16.4 (SpaceStorage)"` | value reported in `server_version` parameter status |
| `protocols.cassandra.cluster_name` | STRING | node name | `system.local.cluster_name` |
| `protocols.cassandra.protocol_versions` | list of `4`,`5` | `4 5` | negotiable versions |
| `protocols.redis.default_container` | IDENT | `redis` | container used by native commands before `SS.USE` |
| `protocols.redis.max_inline_bytes` | SIZE | `64k` | RESP inline/bulk limits |
| `protocols.elasticsearch.version` | STRING | `"8.15.0"` | reported by `GET /` |
| `protocols.clickhouse.display_name` | STRING | node name | `Hello` display name |
| `protocols.clickhouse-http.max_query_size` | SIZE | `256k` | |
| `protocols.s3.virtual_host_suffix` | STRING | — | when set, `Host: <bucket>.<suffix>` is virtual-host addressing |
| `protocols.s3.max_object_size` | SIZE | `5g` | single `PutObject` |
| `protocols.webdav.realm` | STRING | `"SpaceStorage"` | Basic/Digest realm |
| `protocols.<h>.handshake_timeout` | DURATION | `1s` | FR-004 deadline (all handlers) |
| `protocols.<h>.idle_timeout` | DURATION | `10m` | idle session close |
| `protocols.<h>.max_connections` | NUMBER | `0` (unlimited) | per handler |

## New validation codes

| Code | Rule |
|------|------|
| `query_defaults_bad_quorum` | value in vocabulary |
| `query_defaults_timeout_out_of_range` | `1ms..24h` |
| `query_defaults_max_below_default` | `max_timeout >= timeout` |
| `auth_users_file_required` | any client-facing handler declared ⇒ `auth.users_file` present |
| `users_file_unreadable`, `users_file_permissions` (mode > 0600), `users_file_syntax{line}`, `users_file_duplicate_user{name}` | |
| `protocols_unknown_handler{h}` | block name must be a registered handler |
| `protocols_unknown_option{h, o}` | |
| `driver_mapping_missing{handler, type}` | startup check FR-018 (structural) |

## Users file format (interim, R11)

```text
# name        secret          namespace   [schema=public]  [roles=…]
app-a         s3cr3t-a        tenant-a
app-b         s3cr3t-b        tenant-b    schema=analytics
ops           very-secret     -           roles=admin       # '-' = no bound namespace; admin may select any on postgresql/clickhouse/cassandra
```

Fields are whitespace-separated; `#` starts a comment; secrets may be quoted with `"` to contain spaces. For S3 the `name` is the access key id and `secret` the secret key. A principal with `-` namespace and no `admin` role is a configuration error (`users_file_no_namespace{name}`).

## New buffers registered (`buffers { … }`)

| Name | Default | Range | Policy | Owner |
|------|---------|-------|--------|-------|
| `types.memory` | 1 GiB | 16 MiB – 1 TiB | Reject | `02` (interim in-memory inventory; `03` will re-home) |
| `exec.records` | 8 MiB | 1 MiB – 1 GiB | Reject (oldest evicted) | `02` |
| `protocol.<handler>.recv` / `.send` | 64 MiB each | 1 MiB – 64 GiB | Wait | `02` (one pair per declared handler) |

## Effective configuration additions

`EffectiveConfig` gains: `query_defaults { write_quorum, read_quorum, timeout, max_timeout, source }`, `auth { users_file, users: <count> }`, `protocols[]` = `{ handler, declared: bool, entrypoints: [...], connections_active, versions: [...], knobs {...} }` (the **protocol map**, FR-034/FR-041). The CLI gains `spacestorage protocols` (human table / `--output json`) and `spacestorage executions [--protocol …] [--limit N]`.

## Example (starter, also `fixtures/node-all-protocols.conf`)

See [fixtures/node-all-protocols.conf](fixtures/node-all-protocols.conf) — declares `admin`, `admin-http`, all eight protocol handlers, `query_defaults`, and `auth`. Must pass `spacestorage validate` unchanged (SC-010).

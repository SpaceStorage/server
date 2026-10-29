# Configuration

`spacestoraged` reads an nginx-style file. `spacestorage validate` checks the same file and does not start a node.

```text
spacestoraged --config <file> [--set <dotted.path>=<value>]...
spacestorage validate <file> [--set <dotted.path>=<value>]...
```

This page lists the blocks `crates/config/src/resolve.rs` accepts. A name inside a known block that the parser does not read is ignored. An unknown **top-level** block is `unknown_directive` and validate exits 2.

## Grammar

```text
file        := item* EOF
item        := directive | block
directive   := IDENT arg* ';'
block       := IDENT arg* '{' item* '}'
arg         := IDENT | NUMBER | SIZE | DURATION | STRING | PATH
comment     := '#' to end of line
```

Keywords are case-sensitive. Size suffixes `k`, `m`, `g` are binary (KiB, MiB, GiB). Durations use `ms`, `s`, `m`, or `h`.

Reload class is what `GET /v1/config` reports when a running node is compared with a new file. Only the settings in the last column are classified today. Anything else needs a process restart to take effect.

## `node`

| Directive | Meaning | Reload |
|-----------|---------|--------|
| `name` | Node name. Default is the hostname | restart |
| `labels { key value; … }` | Placement labels. Every key on the cluster topology ladder must be present | restart |

## `runtime`

| Directive | Default | Meaning | Reload |
|-----------|---------|---------|--------|
| `threads` | `auto` (one per core) | Worker threads. Integer ≥ 1, or `auto` | restart |
| `drain_timeout` | `30s` | How long drain waits. Range 1s–24h | live |

## `log`

| Directive | Default | Meaning | Reload |
|-----------|---------|---------|--------|
| `level` | `info` | `error`, `warn`, `info`, `debug`, `trace` | live |
| `format` | `text` | `text` or `json` | restart |
| `kafka { brokers …; topic …; }` | off | Ship logs to Kafka | restart |
| `syslog { address …; transport udp; }` | off | Ship logs to syslog | restart |

## `metrics`

Present only when you open a `metrics` block. Omitted means metrics export stays at the built-in defaults.

| Directive | Meaning |
|-----------|---------|
| `audit_log on;` | Audit log switch (`on`, `true`, or `1`) |
| `otel { endpoint URL; interval 30s; }` | OTLP push target and interval |
| `slow_query { enabled on; threshold 1s; }` | Slow-query log |

## `admin` and `disable`

| Directive | Meaning | Reload |
|-----------|---------|--------|
| `admin { token_file PATH; }` | File of the admin bearer token. Required when `admin` or `admin-http` is enabled. The token is never inlined | live (the file is re-read) |
| `disable admin;` | Do not listen for admin TCP | restart |
| `disable admin-http;` | Do not listen for admin HTTP | restart |

Each of `admin` and `admin-http` must be either an entrypoint or disabled. Both enabled and disabled is `admin_handler_conflict`.

## `entrypoint`

Repeatable. The optional name after `entrypoint` defaults to `<handler>@<address>:<port>`.

| Directive | Default | Meaning |
|-----------|---------|---------|
| `address` | `127.0.0.1` | Bind address |
| `port` | required | 1–65535. `(address, port)` must be unique |
| `handler` | required, once | One of the handler names in the [product page](product.md) |
| `plaintext;` | — | No TLS. Every entrypoint must say `plaintext` or contain `tls` |
| `tls { certificate PATH; key PATH; }` | — | PEM paths. Both required. Leaf must be valid at startup |
| `ingest { namespace; container; type; }` | — | Syslog ingest target for an ingest entrypoint. `type` defaults to `log_stream` |

Changing address, port, or handler is restart-required.

## `buffers`

Live. Each line is `name SIZE;`.

| Name | Default | Range | When full |
|------|---------|-------|-----------|
| `net.recv` | 64 MiB | 1 MiB–64 GiB | wait |
| `net.send` | 64 MiB | 1 MiB–64 GiB | wait |
| `request.queue` | 16 MiB | 1 MiB–16 GiB | reject |

`buffer_unknown` and `buffer_out_of_range` fail validate. A warning `buffers_exceed_memory` does not.

## `cluster`

| Directive | Meaning |
|-----------|---------|
| `name` | Cluster name |
| `bootstrap;` | This node bootstraps the cluster |
| `topology_ladder` | Ordered farness keys, for example `az` or `rack az region` |
| `quorum_domain` | Voting set name. Not the same thing as the label `planet` |
| `master_key_file` | Same path as `keys { master_key_file }` |
| `token_file` | Join secret |
| `join;` or `join ADDRESS;` | Join an existing cluster |
| `join_token_file` | Join token file |
| `admin_login` / `admin_password_file` | Bootstrap admin identity. Password is a file, not a literal |
| `product_version N` | Override the compiled product version. `0` is `product_version_zero`. Live for new work |
| `controller_exclusive_data on;` | Voters hold no tenant replicas. `on` without the control-plane feature is `Slice7Required` |
| `starter_namespace NAME;` | Create that namespace at bootstrap if the name is absent. Omitted means no implicit tenant |
| `seeds { name; address; port; }` or `peers { … }` | Other coordinators. Address and port are both required |
| `raft { heartbeat 500ms; election_timeout 2s; }` | `election_timeout` must be greater than `heartbeat`, and both must be greater than 0 |

## `limits`

All optional. Missing fields use the built-in default. A zero value is `limits_zero`. Live for new connections and new queries.

| Directive | Default |
|-----------|---------|
| `max_key` | 1 KiB |
| `max_value` | 16 MiB |
| `max_query_text` | 1 MiB |
| `max_result` | 64 MiB |
| `max_connections_per_entrypoint` | 10000 |
| `max_connections_per_principal` | 1000 |

Oversize input is `limit_exceeded` with the limit name. It is not truncated.

## `query`

Live for new queries. Omitted numeric fields use the defaults below.

| Directive | Default | Meaning |
|-----------|---------|---------|
| `max_concurrent_per_node` | 512 | Node-wide query slots. `0` is `query_max_concurrent_zero` |
| `max_concurrent_per_namespace` | 128 | Per-namespace slots |
| `max_memory` | 256 MiB | Query memory cap |
| `spill on;` / `spill off;` | off, unless the complete-product starter turns it on | Spill sort, hash, and aggregation. Not a substitute for a concurrency slot |
| `default_concurrency` | 1 | Parallelism hint |

## `query_defaults`

| Directive | Product default | Meaning |
|-----------|-----------------|---------|
| `write_quorum` | `TWO` | Durable acknowledgements inside `quorum_domain`. A one-node lab file may set `ONE` |
| `read_quorum` | `ONE` | Read acknowledgements |

## `keys`

| Directive | Meaning |
|-----------|---------|
| `master_key_file PATH;` | Master key file. Key bytes never appear in the config |
| `create_master_if_absent;` | Create the file when it is missing |
| `keyring_file` | Removed. Validate returns `KeyringRemoved`. Use `master_key_file` |
| `external_kms { endpoint; key_name; token; }` | Vault-style unwrap. Complete product |

## `storage`

| Directive | Meaning |
|-----------|---------|
| `data_dir PATH;` | Node data directory |
| `sync` | Durability sync mode string |
| `gc_grace` | Tombstone grace duration |
| `wal_segment SIZE;` | WAL segment size |
| `group_commit { max_wait DURATION; max_bytes SIZE; }` | Group commit bounds |
| `drive ID { path PATH; }` or `drive { id; path; }` | A named data path. The parser stores `id` and `path` |

## `namespace` and `container`

```text
namespace acme {
    quota {
        bytes 10G;
        objects 1000000;
        connections 500;
        ops_per_sec 10000;
    }
    private_metrics off;
    private_logs off;
}
```

A negative quota argument is `QuotaNegative`. `private_metrics on` and `private_logs on` are `ObservabilityRequired` until metrics and logs export are part of the build. Export of those streams belongs to observability, not to this file.

`container NAME { encryption { key "…"; } }` with any key bytes is `KeyMaterialForbidden`. Encryption is declared with a `key_ref`, not with inline key material. That check runs at validate for a `container` block.

## `ingest` and `jobs`

```text
ingest kafka orders {
    namespace acme;
    container events;
    type log_stream;
    format json;
    brokers 127.0.0.1:9092;
    topic orders;
    group spacestorage;
    plaintext;
    # or tls { certificate …; key …; }
}
```

The block form is `ingest kafka NAME { … }`.

| `jobs` directive | Meaning |
|------------------|---------|
| `enabled on;` | Run migration, transform, and backup jobs. The migration-backup build turns this on unless you set it off |
| `catchup_concurrency N;` | Catch-up workers. Minimum 1 |

## Blocks that parse and do nothing else

Top-level `labels`, `memory`, `replication`, and `types` are accepted so older fixtures load. They do not set fields.

## Examples

| File | What it shows |
|------|----------------|
| [`docs/examples/one-node.conf`](../examples/one-node.conf) | One node, PostgreSQL and Redis |
| [`docs/examples/three-node/`](../examples/three-node/) | Three nodes |
| [`docs/examples/node.conf`](../examples/node.conf) | Runtime grammar sample |
| [`docs/examples/compat/`](../examples/compat/) | Limits |
| [`docs/examples/durability/`](../examples/durability/) | Drives |
| [`docs/examples/controlplane/raft-block.conf`](../examples/controlplane/raft-block.conf) | Raft timeouts |
| [`docs/examples/ingest/`](../examples/ingest/) | Kafka and syslog ingest |
| [`docs/examples/namespace-block.conf`](../examples/namespace-block.conf) | Starter namespace |

# Using the server

Work from the repository root (`server/`). Workspace `rust-version` is 1.88.

## Build

```bash
# First binary: PostgreSQL and Redis only. This is the default.
cargo build -p spacestoraged -p spacestorage

# Complete product, including the other five client protocols.
cargo build -p spacestoraged --features complete-product
cargo build -p spacestorage
```

The daemon is `target/debug/spacestoraged`. The CLI is `target/debug/spacestorage`.

## A one-node lab

The first-binary starter is [`docs/examples/first-binary-one-node.conf`](../examples/first-binary-one-node.conf). It expects three secret files and a data directory:

```bash
mkdir -p /tmp/spacestorage/db-1 /tmp/spacestorage/etc
head -c 32 /dev/urandom | base64 > /tmp/spacestorage/etc/admin.token
head -c 32 /dev/urandom > /tmp/spacestorage/etc/master.key
head -c 32 /dev/urandom > /tmp/spacestorage/etc/join.secret
chmod 600 /tmp/spacestorage/etc/admin.token /tmp/spacestorage/etc/master.key /tmp/spacestorage/etc/join.secret
```

Point `admin.token_file`, `cluster.master_key_file`, `cluster.token_file`, and `storage.data_dir` at those paths before starting. The published example uses `/etc/spacestorage` and `/var/lib/spacestorage`.

Check the file, then start:

```bash
spacestorage validate docs/examples/first-binary-one-node.conf
spacestoraged --config docs/examples/first-binary-one-node.conf
```

`--set key=value` overrides a dotted setting for that process. Example: `--set runtime.threads=4`. You cannot add an entrypoint with `--set`.

On this starter the ports are:

| Port | Handler |
|------|---------|
| 5432 | `postgresql` |
| 6379 | `redis` |
| 7700 | `admin` |
| 7701 | `admin-http` |
| 7702 | `internode` |
| 7703 | `replication` |

A longer operator script for slices 1–5 is [`specs/016-mvp-and-nongoals/quickstart.md`](../../specs/016-mvp-and-nongoals/quickstart.md). The automated full-v1 proof (three nodes, Redis, live Raft TCP, L0) is:

```bash
cargo test -p spacestorage-conformance --features complete-product --test quickstart quickstart_full_v1_three_node_raft_and_l0
```

## Clients

PostgreSQL, first binary. `BEGIN`, `COPY`, and `DECLARE` are not in this profile and return `0A000`.

```bash
psql "host=127.0.0.1 port=5432 dbname=postgres"
```

Redis, first binary. `GET`, `SET`, `DEL`, `EXISTS`, `SCAN`, `AUTH`, `PING`, `SELECT`, and `TTL` are in the MUST set. `EVAL` and `CLUSTER` are not.

```bash
redis-cli -h 127.0.0.1 -p 6379
```

On a complete-product build, add an `entrypoint` for each extra handler (`cassandra`, `elasticsearch`, `clickhouse`, `clickhouse-http`, `s3`, `webdav`) and point the stock client at that port. The port number is yours. The handler name is fixed.

## Admin CLI

Most commands call admin HTTP. Pass the URL of the `admin-http` entrypoint. Several subcommands default to `http://127.0.0.1:8080`, which is **not** the port in the first-binary example. Use `--endpoint` to match the config (7701 in the example above).

```bash
export SPACESTORAGE_TOKEN="$(cat /tmp/spacestorage/etc/admin.token)"

spacestorage status --endpoint http://127.0.0.1:7701
spacestorage namespaces --endpoint http://127.0.0.1:7701
spacestorage namespace create acme --endpoint http://127.0.0.1:7701
spacestorage roles --endpoint http://127.0.0.1:7701
```

Exit code 4 from namespace, quota, and role commands means `NotClusterAdmin`, `CascadeRequired`, or `QuotaExceeded`.

| Command | Purpose |
|---------|---------|
| `validate <file>` | Parse and check a config. Exit 2 on a hard error |
| `status` | Node status |
| `namespaces` / `namespace` | List, create, rename, delete, describe |
| `quotas set` | Replace a namespace quota (complete product, `tenancy-quotas`) |
| `roles` | List roles |
| `executions` / `explain` / `jobs` | Query engine |
| `migrate` / `transform` / `backup` / `restore` / `job` | Data movement |
| `controllers` / `raft-status` | Control plane |
| `ui` / `ingest` | Admin UI paths and Kafka or syslog ingest |
| `l0` / `legal` / `cdc` / `composition` / `billing` / `kms` | Tip surfaces |
| `query-stats` | Query counters |

`spacestorage ui` prints the admin-http UI paths. `spacestorage ingest` takes `--token` or `SPACESTORAGE_TOKEN`.

## Namespaces from SQL

On PostgreSQL, `CREATE DATABASE`, `ALTER DATABASE … RENAME`, and `DROP DATABASE` are namespace operations and require the cluster admin. `DROP DATABASE` of a namespace that still has containers returns `CascadeRequired` and does not delete the namespace.

Cassandra `CREATE KEYSPACE` and ClickHouse `CREATE DATABASE` / `RENAME DATABASE` use that same cluster commit. A session stays up if the handler's default database name is not registered. Data statements then return `NamespaceNotFound` until the client presents a name that exists (`USE`, a startup keyspace, or a hello database).

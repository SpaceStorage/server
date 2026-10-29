# SpaceStorage server

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/guide/images/logo-on-dark.svg">
  <img alt="SpaceStorage" src="docs/guide/images/logo.svg" width="72" height="72">
</picture>

SpaceStorage is one Rust process, `spacestoraged`, that stores many datatypes and speaks several client protocols on separate ports. A datatype is the unit of behavior. Protocols are front doors onto the same planner and catalog. There is no sidecar database and no mandatory proxy: every live node accepts client requests.

The admin CLI is a second binary, `spacestorage`. It checks config files and calls the node's admin HTTP API.

## Protocols

One entrypoint is one handler. The port is yours; the handler name is fixed.

| Handler | Client |
|---------|--------|
| `postgresql` | `psql` and other PostgreSQL 3.0 clients |
| `redis` | `redis-cli` (RESP2) |
| `cassandra` | Cassandra native v4 and v5 |
| `elasticsearch` | Elasticsearch HTTP |
| `clickhouse` | ClickHouse native |
| `clickhouse-http` | ClickHouse HTTP |
| `s3` | S3 SigV4, path-style |
| `webdav` | WebDAV |
| `admin` | Admin TCP |
| `admin-http` | Admin HTTP, UI, metrics |
| `internode` | Node-to-node, including Raft |
| `replication` | Replica traffic |

Verbs outside a protocol's supported set return a not-supported error. They do not return an empty success.

## Run

From this directory. Rust **1.88**.

```bash
cargo build -p spacestoraged -p spacestorage
```

That build is the server: every handler above, the admin UI, ingest, and the cluster.

Guide: [what the server is](docs/guide/product.md), [architecture](docs/guide/architecture.md), [start and clients](docs/guide/using.md), [configuration](docs/guide/configuration.md). Example configs are in `docs/examples/`.

## For people changing the server

How the tree is specified and what is still deferred lives in [`AGENTS.md`](AGENTS.md): constitution, `specs/001`–`016`, and the milestone ledger. That is the map for writing the server, not for using it.

# Product

SpaceStorage is one Rust process, `spacestoraged`, that stores many datatypes and speaks several client protocols on separate ports. A datatype is the unit of behavior. Protocols are front doors onto the same planner and catalog. There is no sidecar database and no mandatory proxy: every live node accepts client requests.

The admin CLI is a second binary, `spacestorage`. It validates config files and calls the node's admin HTTP API.

## Protocols

The complete product listens for stock clients on the ports you declare. One entrypoint is one handler.

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

Build with `--features complete-product`. Without that flag the binary refuses `cassandra`, `elasticsearch`, `clickhouse`, `clickhouse-http`, `s3`, and `webdav` at startup.

A verb outside the supported set for that protocol returns a not-supported error. It does not return an empty success. The verb tables are in `specs/015-compatibility-and-limits/contracts/`.

## Also on the node

- Raft between nodes, over the internode port
- Admin commands `spacestorage l0`, `legal`, `cdc`, `composition`, `billing`, `kms`
- Admin HTTP under `/v1/l0`, `/v1/legal`, `/v1/cdc`, `/v1/compositions`, `/v1/billing`, `/v1/kms`

Not in this build: invoice export, starting a cluster of separate OS processes from one command, and further UI polish past login.

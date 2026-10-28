# Product

SpaceStorage is one Rust process, `spacestoraged`, that stores many datatypes and speaks several client protocols on separate ports. A datatype is the unit of behavior. Protocols are front doors onto the same planner and catalog. There is no sidecar database and no mandatory proxy: every live node accepts client requests.

The admin CLI is a second binary, `spacestorage`. It validates config files and calls the node's admin HTTP API.

## Protocols

The complete product listens for stock clients on the ports you declare. One entrypoint is one handler.

| Handler | Client | First binary | Complete product |
|---------|--------|:------------:|:----------------:|
| `postgresql` | `psql` and other PostgreSQL 3.0 clients | yes | yes |
| `redis` | `redis-cli` (RESP2) | yes | yes |
| `cassandra` | Cassandra native v4 and v5 | no | yes |
| `elasticsearch` | Elasticsearch HTTP | no | yes |
| `clickhouse` | ClickHouse native | no | yes |
| `clickhouse-http` | ClickHouse HTTP | no | yes |
| `s3` | S3 SigV4, path-style | no | yes |
| `webdav` | WebDAV | no | yes |
| `admin` | Admin TCP | yes | yes |
| `admin-http` | Admin HTTP, UI, metrics | yes | yes |
| `internode` | Node-to-node, including Raft | yes | yes |
| `replication` | Replica traffic | yes | yes |

A first-binary build refuses the names `cassandra`, `elasticsearch`, `clickhouse`, `clickhouse-http`, `s3`, and `webdav` at startup (`entrypoint_unknown_handler`).

Each protocol implements a documented MUST set. Verbs outside that set return a not-supported error for that protocol. They do not return an empty success. The verb tables live in `specs/015-compatibility-and-limits/contracts/`.

## Profiles

Build **`complete-product`** on both `spacestoraged` and the `spacestorage` CLI. That is the product in [`009-full-product`](../milestones/009-full-product.md): every handler in the table above, slices 1–11, live Raft over internodes TCP, and the later mounts (L0, legal-hold, named CDC, external KMS, N/N+1, planetary composition, billing estimate).

`first-binary` and `handlers-complete` remain Cargo features so the older slice gates still compile. A plain `cargo build -p spacestoraged` still selects `first-binary`.

Merge gate:

```bash
cargo test -p spacestorage-conformance --features complete-product
```

## What the tip includes

Recorded in [`009-full-product`](../milestones/009-full-product.md):

- Raft peer RPC over internodes TCP
- `spacestorage l0`, `legal`, `cdc`, `composition`, `billing`, `kms`
- Admin HTTP paths under `/v1/l0`, `/v1/legal`, `/v1/cdc`, `/v1/compositions`, `/v1/billing`, `/v1/kms`

Still deferred, and not unpaid slices: invoicing export, multi-process orchestration beyond the harnesses, further UI polish, WAL re-read cases past the Redis gate, and auto-bootstrapping the control plane on every membership start.

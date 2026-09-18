---
speckit_command: specify
suggested_slug: mvp-and-nongoals
source: gap-analysis-2026-09-14
read_after: 00-constitution.md
---

# Feature: MVP cut, sequencing, and product non-goals

Specify what to build first, what the complete product still owes, and what SpaceStorage is not. This is a product-sequencing document, not a substitute for `01`–`15`.

## What

The constitution and `01`–`15` describe the **complete** product. Implementation MUST NOT treat that complete surface as a single release. This file defines slices.

### Sequencing (build order)

Numeric intent order stays the specify order. **Implementation** of a slice MAY skip later UI/ingest work. Suggested slices:

1. **Runtime** (`01`): process, entrypoints, admin CLI/HTTP, buffers, drain.
2. **Types + durability on one node** (`03`, `13`): creatable containers, WAL, restore, master-key file (`14` subset).
3. **One protocol** (`02` PostgreSQL handler at the **first-binary** subset in this file) over those types.
4. **Second protocol** (Redis) proving the `15` Redis MUST list on `K/V Store` and canonical blob fallback.
5. **Membership + internode + clocks** (`11`, `12`) and **three-node quorum** (`04` replication/quorum only).
6. **Remaining protocol handlers** at the `15` MUST subset (Cassandra, Elasticsearch, ClickHouse native+HTTP, S3, WebDAV).
7. **Control plane Raft** (`06`), tenancy quotas (`07`), full authz vocabulary (`14`).
8. **Query engine beyond CRUD** (`05`): joins, aggregation, MapReduce, subscribe, distributed transactions.
9. **Observability catalog** (`08`) live on `/metrics` (series MAY appear earlier as they are implemented).
10. **Migration/transforms** (`10`) and **backup/PITR** (`13`).
11. **Admin UIs and Kafka/syslog ingest** (`09`) — after the data path is operable without them.

Slices **1–5** are the **first shippable binary**. Do not call that binary a "v1" of the seven-protocol matrix. Slices 6–11 complete the intent surface (`01`–`15`).

### First shippable binary (definition of done)

MUST include:

- Single-node and three-node topologies from starter examples (`04`); cluster topology ladder default **`[az]`** (one value is enough on a single node)
- Types: `K/V Store`, `Relational Table`, `Document Store` only (no Cassandra/ES/ClickHouse/S3/WebDAV **handlers** yet)
- **PostgreSQL** smoke: wire 3.0, SCRAM, simple + extended/prepared, `INSERT`/`SELECT`/`UPDATE`/`DELETE`, `CREATE`/`DROP` table, **auto-commit**. **No `COPY`, no `BEGIN`.**
- **Redis** smoke: the complete-product Redis MUST list in `15` **on `K/V Store` only** (`AUTH`, `PING`, `GET`, `SET`, `DEL`, `EXISTS`, `SCAN`, `SELECT` as no-op, TTL mapped). Other types: canonical blob only; no type-specific Redis verbs.
- Leaderless replication in an explicit **`quorum_domain`**, write `ack==2` / read `ack==1` defaults **inside the source domain**, durable WAL acks (`12`, `13`)
- Stable node identity, UUID + join secret, bootstrap/join (`11`)
- `internode` and `replication` **always listening**; default bind **loopback**; a cluster address MUST be bound to join remotes (`12`)
- Every entrypoint **`tls {…}` or `plaintext;`** — omitted is a startup error (`01`, `14`)
- Admin CLI/HTTP, drain
- Global `/metrics` with the labels that exist for implemented paths (`08`)
- Cluster **master-key file** wrapping namespace KEKs in controller storage (`14`); no required external KMS
- Catalog flag `multi_active` **default off**; create with `on` **refused**

MUST NOT be required for the first binary (still required for the complete product unless listed as product non-goals below):

- Remaining protocol handlers at the `15` complete-product MUST subset (slice 6)
- Cerebro-like and Kibana-like UIs
- Kafka and syslog **ingest**
- MapReduce / shuffle across regions
- All L0 data structures as day-to-day product surface (they remain in the catalog; L3 defaults suffice)
- Planetary / multi-continent placement examples in production (`planet` is a **label**; `quorum_domain` is already required)
- External KMS
- Mixed-version rolling upgrade (MAY land with `15` once format versions exist)
- `BEGIN` / `COPY` on PostgreSQL; Redis type-specific verbs off `K/V Store`

### Product non-goals (not "later"; not SpaceStorage)

These MUST NOT be implied by the type or protocol inventories:

- A second query engine per protocol
- Drop-in replacement for every feature of PostgreSQL, Cassandra, Redis, Elasticsearch, ClickHouse, AWS S3, or a generic WebDAV server (see `15` MUST NOT column)
- Kafka as a stored log product (Kafka is ingest and outbound logging; Log Stream is the type)
- Multi-master conflict that waits for a human to pick a value (LWW/merge only)
- Byzantine / adversarial node model
- Per-tenant CPU hard isolation (`15`)
- A SpaceStorage-native client protocol (constitution MAY later)
- SQL `SERIALIZABLE` isolation (`15`)
- `multi_active=on` in the first binary (catalog flag exists, create refused)

### Explicit "later, not first binary" (still in intent)

- `09` UIs and ingest
- Full L0 creatable structures as a supported user workflow (catalog + conformance MAY still test them)
- Federated / union / materialized view at planetary scale
- Billing money formula (metrics for billing remain; invoicing is out of this repo)
- GDPR/legal-hold workflows (residency is labels + placement; erase is delete/tombstone/`10`)
- CDC as a named product beyond Log Stream + WAL

## Why

Implementing L0–L4 × eight handlers × MapReduce × UIs × planetary replication in one step will lock in guesses and never ship. Sequencing keeps the constitution intact while making a testable first binary (slices 1–5).

## Actors

- Maintainer choosing the next slice to specify/implement
- Early operator running a three-node KV/SQL/document cluster
- Application team on PostgreSQL or Redis clients
- Later: tenant using UIs and ingest

## Requirements

- Build order and first-binary definition above are product requirements for planning, not optional commentary. The first binary is slices 1–5, not the seven-protocol matrix.
- Complete-product obligations in `01`–`15` remain; skipping a slice in an **implementation** milestone MUST be recorded as deferred, not deleted from intent.
- Non-goals MUST NOT appear as silent "we'll see" in specs; they MUST be Out of Scope there.

## Out of scope for this feature

- The contents of `01`–`15` themselves
- Marketing version numbers beyond the slice names
- CI/CD of this repository

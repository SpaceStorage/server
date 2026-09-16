---
speckit_command: specify
suggested_slug: query-execution
source: server/start
read_after: 00-constitution.md
---

# Feature: Query execution, MapReduce, transactions, and fault-tolerant results

Specify the execution layer so SpaceStorage can parse, plan, and run queries natively for all protocols, including parallel distributed processing.

## What

SpaceStorage MUST implement:

- Query parser
- Query planner
- Task scheduler
- Task executor
- MapReduce engine
- Shuffle engine
- Aggregation engine
- Join engine
- Data exchange
- Execution state

MapReduce exists to process data in parallel and in a distributed way.

This stack MUST work **natively for all protocols** (Elasticsearch, PostgreSQL, and so on).

Queries MUST implement **concurrency level** if it is needed, specified, and implemented in the query planner. Admission limits (max concurrent queries, max query memory, spill or refuse) are in `15`.

Transactions MUST implement **isolation level** if it is needed, specified, and implemented in the query planner. The allowed set is **not open**: `READ COMMITTED` (SQL default) and `SNAPSHOT` where the type supports it. **`SERIALIZABLE` is a product non-goal** (`15`, `16`).

The canonical execution representation is **one IR owned by this feature**. Protocol parsers (`02`) translate into that IR; they MUST NOT each keep a private engine. PostgreSQL-shaped SQL and ClickHouse SQL are the SQL dialects (`15`); CQL, Redis commands, ES DSL, S3/WebDAV verbs map to the same IR.

Client disconnect and explicit cancel MUST stop the query (`15`). Subscribe-on-results remains required for async completion.

Prepared statements, COPY, and EXPLAIN (logical plan at least on SQL protocols) MUST be executable through this stack when the protocol's MUST subset includes them (`15`).

The database MUST implement **fault tolerance**: if all replicas holding a part of the data are down, answer the user that this part of the data is not available now.

It MUST have an interface to **subscribe on results**, implementing asynchronous performance of queries when needed.

All queries MUST be accompanied with **timeout** and **quorum level** as in Cassandra. If the driver does not support this, use the global system defaults.

When choosing among replicas or shuffle workers that already satisfy the requested quorum, the planner MUST rank by the cluster **topology ladder** (first differing key = farther) and MUST **prefer measured internode RTT** once samples exist (`04`, `12`). A missing ladder value is not local. `LOCAL_*` is the coordinator's `quorum_domain`, not “same AZ”.

L1 distributed transactions (from the shared layer) MUST be usable from this execution layer when the planned query requires them.

## Why

Users issue SQL, CQL, Redis commands, ES queries, ClickHouse SQL, S3/WebDAV operations — and get one planner/executor that understands SpaceStorage types, composition, and placement, including heavy MapReduce jobs.

## Actors

- Client issuing a query through any supported protocol
- Query planner choosing concurrency, isolation, timeout, quorum
- Task scheduler/executor running MapReduce, joins, aggregations, shuffle
- Subscriber waiting asynchronously for results
- Client receiving a partial-unavailability error when a shard’s replicas are all down

## Requirements

- Native protocol integration (not a lowest-common-denominator SQL-only engine); one canonical IR.
- Planner-owned concurrency and isolation from the closed set in `15`.
- Timeout + quorum on every query; defaults if the driver cannot pass them.
- Subscribe API for async query completion; cancel on disconnect.
- Explicit unavailability response when a data part has zero live replicas.
- MapReduce/shuffle/join/aggregation as listed; shuffle uses `internode` (`12`).
- Replica/shuffle locality: ladder rank, then RTT; `LOCAL_*` is `quorum_domain` (`04`, `12`).

## Out of scope for this feature

- Protocol listen ports (`02`)
- Type inventories except as inputs to planning (`03`)
- Admin visualization of shards (`09`)
- Wire-level MUST/MUST NOT verb matrix (`15`) except isolation/dialect/admission consumed here
- Internode framing (`12`) except using it for shuffle/data-exchange

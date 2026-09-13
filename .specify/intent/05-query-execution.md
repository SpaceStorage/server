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

Queries MUST implement **concurrency level** if it is needed, specified, and implemented in the query planner.

Transactions MUST implement **isolation level** if it is needed, specified, and implemented in the query planner.

The database MUST implement **fault tolerance**: if all replicas holding a part of the data are down, answer the user that this part of the data is not available now.

It MUST have an interface to **subscribe on results**, implementing asynchronous performance of queries when needed.

All queries MUST be accompanied with **timeout** and **quorum level** as in Cassandra. If the driver does not support this, use the global system defaults.

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

- Full execution-layer inventory above.
- Native protocol integration (not a lowest-common-denominator SQL-only engine).
- Planner-owned concurrency and isolation when specified.
- Timeout + quorum on every query; defaults if the driver cannot pass them.
- Subscribe API for async query completion.
- Explicit unavailability response when a data part has zero live replicas.

## Out of scope for this feature

- Protocol listen ports (`02`)
- Type inventories except as inputs to planning (`03`)
- Admin visualization of shards (`09`)

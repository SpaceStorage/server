---
speckit_command: specify
suggested_slug: observability
source: server/start
read_after: 00-constitution.md
---

# Feature: Observability, billing metrics, and logging

Specify SpaceStorage monitoring and logging. Metric names follow **Prometheus naming convention**. Main systems: **OpenTelemetry** and **Prometheus**. Methodology: **Four Golden Signals**.

Server statistics are collected and stored **in memory for each node**. All types of data count access, hits/misses ratio, duration, etc.

Each node calculates metrics only for **its own datatypes**. Shared datatypes are **aggregated by the primary server** and stored in memory.

Buffer sizes are configured per node; **usage of each buffer MUST be monitored**.

Metrics are also used for **billing and quota management**.

Each namespace MAY expose metrics relevant only to that namespace to its own monitoring system, separately from the primary `/metrics` endpoint (personal API endpoint per namespace, or OpenTelemetry push).

Logging MAY be sent to systems such as **Kafka, syslog, etc.** Each namespace MAY send logs to its own logging system separately from the primary logging system. Result: **per-namespace** statistics/logging and **global** statistics/logging for the SpaceStorage administrator.

Preserve every label and series below. They are product requirements.

## Query-processing metrics

Labels:

- kind of query (get/set/delete/update/scan/query/etc.)
- namespace name
- schema name
- datatype name
- node name if applicable
- storage_type (memory|nvme|ssd|hdd|etc.)
- drive name or memory name
- error type if applicable

Expose:

- blocks reads/writes
- bytes reads/writes
- query totals (all queries total and errors total)
- rows/documents/values returned
- hits and misses counts
- lookup probes (B+tree lookup depth, linked list lookup steps, hash table collision probes, etc.)
- index scanning and full scans
- histogram of time of query execution
- histogram of time of query stay in queue
- histogram of time of query stay for waiting node
- histogram of result size in bytes
- retries total
- query in flight (executing at this moment)

## User/application communication metrics

Labels:

- namespace name
- schema name
- datatype name
- node name if applicable
- user name
- application name
- protocol name
- error type if applicable

Expose:

- query totals (all queries total and errors total)
- status of communication (success/error)
- histogram of time of communication `read()` and `write()` operations
- connection count (for each connection attempt) and connection errors total
- actual connections gauge

## Replication metrics

Labels:

- namespace name
- schema name
- datatype name
- node name if applicable
- drive name or memory name
- protocol name
- error type if applicable

Expose:

- status of replication
- replication lag (in seconds and, if possible, last replicated block)
- queue of replication in blocks, bytes, and objects
- queue of objects waiting for replication
- histogram of waiting time of replication of each block
- replication retries total
- replication throughput
- replication failures

## System metrics

Labels:

- namespace name
- schema name
- datatype name
- node name if applicable

Expose:

- system uptime
- buffer usage (in bytes and percentage)
- cache buffer hits and misses counts
- cache eviction total
- buffer limit hits
- worker threads count
- db storage read duration_seconds
- db storage write duration_seconds
- db storage fsync duration_seconds
- db storage flush duration_seconds
- db storage read bytes
- db storage write bytes
- db storage fsync bytes
- db storage flush bytes
- dns resolution duration_seconds
- dns resolution requests and errors total
- worker threads usage (percentage)
- worker threads usage (count of threads that are busy)
- node_ready
- node_state (starting, ready, draining, degraded, recovering, failed, etc.)
- leader elections total (and errors total)
- histogram of leader election duration

## API metrics

Labels:

- namespace name
- schema name
- datatype name
- node name if applicable
- user name
- application name
- protocol name
- error type if applicable

Expose:

- connections
- requests
- request errors
- histogram of request duration
- active connections gauge

## Background jobs

Labels:

- job name
- job status (starting, running, completed, failed, etc.)
- job duration_seconds
- job errors total
- job retries total
- job queue size
- job queue duration_seconds
- job queue errors total

Expose:

- job queue size
- job queue duration_seconds
- job queue errors total
- job queue retries total
- job queue completed total
- job queue failed total
- job queue starting total
- job queue running total
- job queue completed duration_seconds

These metrics cover:

- compaction
- flush
- checkpoint
- vacuum
- GC
- index rebuild
- replication
- backup
- cleanup
- TTL expiration
- snapshot
- data transformation
- data migration
- data backup
- data restore

## Datatype-specific metrics

- `db_lsm_memtable_size_bytes`
- `db_lsm_sstable_count`
- `db_lsm_compaction_total`
- `db_lsm_compaction_duration_seconds`
- `db_lsm_compaction_pending`
- `db_lsm_tombstones`
- `db_hnsw_nodes`
- `db_hnsw_edges`
- `db_hnsw_search_duration_seconds`
- `db_hnsw_search_ef`

## Durability metrics

- `db_wal_bytes_total`
- `db_wal_fsync_total`
- `db_wal_fsync_duration_seconds`
- `db_checkpoint_total`
- `db_checkpoint_duration_seconds`
- `db_dirty_blocks`
- `db_dirty_bytes`
- `db_unflushed_bytes`
- `db_wal_lag_bytes`
- `db_wal_replay_duration_seconds`
- `db_wal_recovery_duration_seconds`
- `db_recovery_total`
- `db_recovery_duration_seconds`
- `db_recovery_records_total`

## Why

Operators, tenants, and billing share one metric/log model. Four Golden Signals plus datatype- and durability-specific series make cluster health, SLO, and quota enforceable.

## Actors

- Cluster administrator scraping global `/metrics` or OTel
- Tenant scraping namespace-only metrics or receiving OTel push
- Tenant shipping logs to their Kafka/syslog
- Billing/quota engine consuming namespace metrics
- Node process emitting in-memory stats

## Out of scope for this feature

- Cerebro/Kibana UIs that *display* these metrics (`09`)
- Defining who the primary aggregator is (`06`)

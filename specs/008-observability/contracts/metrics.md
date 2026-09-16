# Contract: Who increments which series

**Feature**: `008-observability` | Exposition: this crate | Increments: owners

This crate increments: `spacestorage_shared_aggregation_*`, `spacestorage_log_export_errors_total`, `spacestorage_otel_export_errors_total`, DNS series if this process performs DNS for sinks.

| Owner | Series families |
|-------|-----------------|
| `001` node | uptime, node_ready/state, workers, buffers, entrypoint connections |
| `002` handlers | comm_*, api_* |
| `004`/`012` | replication_* |
| `005` exec | query_* |
| `006` controlplane | raft_leader_election_* |
| `007` tenancy | namespace_usage_*, quota_exceeded_total |
| `013` durability | db_wal_*, db_checkpoint_*, db_dirty_*, db_unflushed_*, db_recovery_* |
| types/LSM/HNSW | db_lsm_*, db_hnsw_* |
| job owners (FR-012) | spacestorage_job_* |
| `008` | freshness, export errors, DNS for exporters |

Do not rename owner-reserved names. Help text lives in `catalog.rs`.

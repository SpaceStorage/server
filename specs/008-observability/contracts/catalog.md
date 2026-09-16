# Contract: Metric catalog (names, labels, types, buckets)

**Feature**: `008-observability` | Spec: FR-001–FR-015, FR-018 | Constitution observability contract

Plans MAY **add** series. MUST NOT **rename or drop** rows below without a constitution amendment. Intent identifiers that already look like Prometheus names are kept. Prose families map to `spacestorage_*`.

**Omit rule (FR-023)**: if a label column is “when known”, skip the key when unknown.

**Duration buckets** (seconds): `0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10, 30, 60`.

**Size buckets** (bytes): `64, 256, 1024, 4096, 16384, 65536, 262144, 1048576, 4194304, 16777216, 67108864`.

## Query-processing (FR-006)

Labels: `kind`, `namespace`, `schema`, `datatype`, `node` (when known), `storage_type` (`memory`\|`nvme`\|`ssd`\|`hdd`\|other documented), `drive` (drive or memory name), `error_type` (errors only).

| Prometheus name | Type | Intent |
|-----------------|------|--------|
| `spacestorage_query_blocks_read_total` | counter | blocks reads |
| `spacestorage_query_blocks_written_total` | counter | blocks writes |
| `spacestorage_query_bytes_read_total` | counter | bytes reads |
| `spacestorage_query_bytes_written_total` | counter | bytes writes |
| `spacestorage_query_total` | counter | query totals |
| `spacestorage_query_errors_total` | counter | errors total |
| `spacestorage_query_rows_returned_total` | counter | rows/documents/values returned |
| `spacestorage_query_hits_total` | counter | hits |
| `spacestorage_query_misses_total` | counter | misses |
| `spacestorage_query_lookup_probes_total` | counter | lookup probes |
| `spacestorage_query_index_scans_total` | counter | index scanning |
| `spacestorage_query_full_scans_total` | counter | full scans |
| `spacestorage_query_duration_seconds` | histogram | execution time |
| `spacestorage_query_queue_duration_seconds` | histogram | queue time |
| `spacestorage_query_wait_node_duration_seconds` | histogram | waiting node |
| `spacestorage_query_result_size_bytes` | histogram | result size |
| `spacestorage_query_retries_total` | counter | retries |
| `spacestorage_query_in_flight` | gauge | in flight |

Owner crate: `exec` (`005`). `kind` examples: `get` `set` `delete` `update` `scan` `query`.

## User/application communication (FR-007)

Labels: `namespace`, `schema`, `datatype`, `node` (when known), `user`, `application`, `protocol`, `error_type` (when known).

| Prometheus name | Type | Intent |
|-----------------|------|--------|
| `spacestorage_comm_query_total` | counter | query totals |
| `spacestorage_comm_query_errors_total` | counter | errors |
| `spacestorage_comm_status` | gauge | 1 success / 0 error on in-flight (also increment totals) |
| `spacestorage_comm_read_duration_seconds` | histogram | `read()` time |
| `spacestorage_comm_write_duration_seconds` | histogram | `write()` time |
| `spacestorage_comm_connect_attempts_total` | counter | connection attempts |
| `spacestorage_comm_connect_errors_total` | counter | connection errors |
| `spacestorage_comm_connections` | gauge | actual connections |

Owner: protocol handlers (`002`) + `node`.

## Replication (FR-008)

Labels: `namespace`, `schema`, `datatype`, `node` (when known), `drive`, `protocol`, `error_type` (when known).

| Prometheus name | Type | Intent |
|-----------------|------|--------|
| `spacestorage_replication_status` | gauge | 1 healthy / 0 failed |
| `spacestorage_replication_lag_seconds` | gauge | lag seconds |
| `spacestorage_replication_last_block` | gauge | last replicated block when known; **omit series** if not possible |
| `spacestorage_replication_queue_blocks` | gauge | queue blocks |
| `spacestorage_replication_queue_bytes` | gauge | queue bytes |
| `spacestorage_replication_queue_objects` | gauge | queue objects |
| `spacestorage_replication_objects_waiting` | gauge | objects waiting |
| `spacestorage_replication_wait_block_duration_seconds` | histogram | wait per block |
| `spacestorage_replication_retries_total` | counter | retries |
| `spacestorage_replication_throughput_bytes_total` | counter | throughput |
| `spacestorage_replication_failures_total` | counter | failures |

Owner: `004` / `012`.

## System (FR-009) — includes `001` reserved names

Labels: `namespace`, `schema`, `datatype`, `node` when they apply. Node-global series omit `namespace`.

| Prometheus name | Type | Intent / prior plan |
|-----------------|------|---------------------|
| `spacestorage_uptime_seconds` | gauge | uptime (`001`) |
| `spacestorage_buffer_usage_bytes` | gauge | buffer usage bytes `{buffer}` (`001`) |
| `spacestorage_buffer_usage_ratio` | gauge | buffer percent as 0–1+ (`001`) |
| `spacestorage_buffer_capacity_bytes` | gauge | `{buffer}` (`001`) |
| `spacestorage_buffer_limit_hits_total` | counter | buffer limit hits (`001`) |
| `spacestorage_cache_hits_total` | counter | cache hits |
| `spacestorage_cache_misses_total` | counter | cache misses |
| `spacestorage_cache_evictions_total` | counter | cache eviction |
| `spacestorage_worker_threads` | gauge | worker thread count (`001`) |
| `spacestorage_worker_threads_busy` | gauge | busy count (`001`) |
| `spacestorage_worker_threads_usage_ratio` | gauge | usage percent as 0–1 |
| `spacestorage_storage_read_duration_seconds` | histogram | db storage read |
| `spacestorage_storage_write_duration_seconds` | histogram | db storage write |
| `spacestorage_storage_fsync_duration_seconds` | histogram | db storage fsync |
| `spacestorage_storage_flush_duration_seconds` | histogram | db storage flush |
| `spacestorage_storage_read_bytes_total` | counter | db storage read bytes |
| `spacestorage_storage_write_bytes_total` | counter | db storage write bytes |
| `spacestorage_storage_fsync_bytes_total` | counter | db storage fsync bytes |
| `spacestorage_storage_flush_bytes_total` | counter | db storage flush bytes |
| `spacestorage_dns_resolution_duration_seconds` | histogram | DNS resolution |
| `spacestorage_dns_requests_total` | counter | DNS requests |
| `spacestorage_dns_errors_total` | counter | DNS errors |
| `node_ready` | gauge | 1 when ready (`001` `spacestorage_node_ready` is an **alias** — expose **both** until a constitution rename; scrapes MUST include `node_ready`) |
| `node_state` | gauge | enum mapped to values: `starting=0 ready=1 draining=2 degraded=3 recovering=4 failed=5` plus `spacestorage_node_state` from `001` with the same mapping |
| `spacestorage_raft_leader_elections_total` | counter | leader elections (`006`) `{raft_group}` |
| `spacestorage_raft_leader_election_errors_total` | counter | election errors |
| `spacestorage_raft_leader_election_duration_seconds` | histogram | election duration |

Also `001`: `spacestorage_entrypoint_connections_active{entrypoint,handler}`.

## API (FR-010)

Labels: `namespace`, `schema`, `datatype`, `node` (when known), `user`, `application`, `protocol`, `error_type` (when known).

| Prometheus name | Type |
|-----------------|------|
| `spacestorage_api_connections_total` | counter |
| `spacestorage_api_requests_total` | counter |
| `spacestorage_api_request_errors_total` | counter |
| `spacestorage_api_request_duration_seconds` | histogram |
| `spacestorage_api_connections_active` | gauge |

Owner: `002` handlers.

## Background jobs (FR-011, FR-012)

Labels: `job` (name), `status` (`starting`\|`running`\|`completed`\|`failed`).

| Prometheus name | Type |
|-----------------|------|
| `spacestorage_job_duration_seconds` | histogram |
| `spacestorage_job_errors_total` | counter |
| `spacestorage_job_retries_total` | counter |
| `spacestorage_job_queue_size` | gauge |
| `spacestorage_job_queue_duration_seconds` | histogram |
| `spacestorage_job_queue_errors_total` | counter |
| `spacestorage_job_queue_retries_total` | counter |
| `spacestorage_job_queue_completed_total` | counter |
| `spacestorage_job_queue_failed_total` | counter |
| `spacestorage_job_queue_starting_total` | counter |
| `spacestorage_job_queue_running` | gauge |
| `spacestorage_job_queue_completed_duration_seconds` | histogram |

`job` values MUST include: `compaction`, `flush`, `checkpoint`, `vacuum`, `gc`, `index_rebuild`, `replication`, `backup`, `cleanup`, `ttl_expiration`, `snapshot`, `data_transformation`, `data_migration`, `data_backup`, `data_restore`. Behavior owners remain `13`/`04`/`12`/`03`/`05`/`10`.

## Datatype-specific (FR-013) — names frozen

`db_lsm_memtable_size_bytes`, `db_lsm_sstable_count`, `db_lsm_compaction_total`, `db_lsm_compaction_duration_seconds`, `db_lsm_compaction_pending`, `db_lsm_tombstones`, `db_hnsw_nodes`, `db_hnsw_edges`, `db_hnsw_search_duration_seconds`, `db_hnsw_search_ef`.

Labels when known: `namespace`, `schema`, `datatype`, `node`.

## Durability (FR-014) — names frozen

`db_wal_bytes_total`, `db_wal_fsync_total`, `db_wal_fsync_duration_seconds`, `db_checkpoint_total`, `db_checkpoint_duration_seconds`, `db_dirty_blocks`, `db_dirty_bytes`, `db_unflushed_bytes`, `db_wal_lag_bytes`, `db_wal_replay_duration_seconds`, `db_wal_recovery_duration_seconds`, `db_recovery_total`, `db_recovery_duration_seconds`, `db_recovery_records_total`.

Owner: `013`.

## Billing / quota (`007` reserved, FR-015)

Do not rename: `spacestorage_namespace_usage_bytes`, `spacestorage_namespace_usage_objects`, `spacestorage_namespace_connections`, `spacestorage_quota_exceeded_total`. Usage series MUST keep `namespace` and `datatype` when applicable.

## Aggregation freshness (FR-024, additive)

| Prometheus name | Type | Labels |
|-----------------|------|--------|
| `spacestorage_shared_aggregation_up` | gauge | `namespace`, `datatype` |
| `spacestorage_shared_aggregation_last_success_timestamp_seconds` | gauge | `namespace`, `datatype` |

## Log / OTel export (this crate)

| Prometheus name | Type | Labels |
|-----------------|------|--------|
| `spacestorage_log_export_errors_total` | counter | `sink` (`kafka`\|`syslog`), `stream` (`global`\|namespace name), `channel` |
| `spacestorage_otel_export_errors_total` | counter | `stream` (`global`\|namespace name) |

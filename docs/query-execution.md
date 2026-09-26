# Query execution

SpaceStorage runs **one** shared planner/executor (`crates/query` / `PlannerEngine`).
Protocol handlers lower wire requests to `LogicalRequest` and call `QueryEngine` —
never a second engine per protocol.

## Stages

`Received` → `Bound` → `Planned` → `Scheduled` → `Running` → `Finalizing` → `Done`
(or `Failed` / `Cancelled`). `EXPLAIN` stops at `Planned` without touching data.
Execution records always name `engine = "planner"`.

## Isolation (closed set)

| Level | Notes |
|-------|--------|
| `READ COMMITTED` | Default |
| `SNAPSHOT` | When the type declares snapshot support |
| `SERIALIZABLE` | Refused (`NotSupported`) — product non-goal |

Cassandra consistency is never rewritten into SQL isolation.

## Admission defaults

| Knob | Default |
|------|---------|
| `max_concurrent_per_node` | 512 |
| `max_concurrent_per_namespace` | 128 |
| `max_memory` | 256MiB |
| `spill` | off |
| `default_concurrency` | 1 (sequential) |

Live-reload applies to **new** queries only. Spill lives under `{data_dir}/spill/{exec_id}/`
and is not restored across restart.

## Ranking

Replica and shuffle workers: topology **ladder distance**, then RTT (`Some` before `None`),
then deprioritize HLC-skew unhealthy peers. Missing ladder = farthest.

## Slice 8 (`query-distributed`)

Join (inner/left), aggregation, MapReduce/shuffle over internode, subscribe-on-results
(job id poll — not `LISTEN`/`NOTIFY`), and complete-product `BEGIN`/`COPY`/cursors.

# Contract: Join, Aggregation, MapReduce, Shuffle

**Feature**: `005-query-execution` | Spec: FR-026–FR-029, FR-035 | Feature flag: `query-distributed` (slice 8)

## Join (FR-028)

| Kind | Semantics |
|------|-----------|
| Inner | SQL inner join on equality of documented join keys |
| Left | SQL left outer; unmatched right is null |

Unlisted kinds (full outer, cross, non-equi nested arbitrarily) → `NotSupported`. Physical: hash join if build fits memory or spill; else nested-loop when the inner side is point-indexed. EXPLAIN names `HashJoin` or `NestedLoopJoin`.

## Aggregation (FR-028)

Documented functions: `COUNT`, `SUM`, `MIN`, `MAX`, `AVG` with optional `GROUP BY` keys. Multi-shard: partial agg per shard, shuffle by grouping key, final agg. HAVING is a post-filter `Expr`.

## MapReduce + shuffle (FR-026, FR-027, FR-029)

Internodes RPCs (additive on `012` handler, not a client port, not `admin`):

| Message | Purpose |
|---------|---------|
| `ShuffleOffer { job, partition }` | reducer announces |
| `ShufflePush { job, partition, bytes }` | map output |
| `ShufflePull { job, partition }` | reducer fetch |
| `StageAbort { job, stage }` | cancel |

Failed stage: retry while query timeout remains; else `StageFailed{stage}` — never hang.

## Elasticsearch ceiling (FR-035)

Allowed to lower: `terms` buckets + metric `min/max/sum/avg/value_count` (`002` R5). Anything else in the ES agg tree → not-supported, no approximate histogram/pipeline/matrix aggs.

## First binary

This contract is **not** required for slices 1–5. `LocalEngine` nested-loop join in `002` tests is not the complete-product join engine.

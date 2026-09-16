# Contract: Coordination, Ranking, Unavailability

**Feature**: `005-query-execution` | Spec: FR-007, FR-010–FR-013, FR-017, FR-031–FR-033 | Owners of arithmetic: `004`/`012`

This crate **does not** implement quorum math, internodes framing, or replica health. It attaches levels, ranks candidates, and translates coordinator outcomes into `ExecError`.

## Timeout and quorum attach (FR-007)

Precedence and clamp/reject are `002` `query-options.md` + live `PlacementInfo` from `004`:

- Default-sourced unsatisfiable → clamp, record `applied.quorum.clamped`.
- Explicit (query/session) unsatisfiable → `QuorumUnsatisfiable` before data.
- `LOCAL_*` = coordinator `quorum_domain`, not a ladder key.

Timeout values come from `query_defaults` / session / query. The **error SLA** is deadline + 1 s (FR-012). No constant assumes a short RTT (R17).

Partial-results (`partial_ok`) uses the same precedence family; syntax in [query-options-additions.md](query-options-additions.md). Built-in default **off**.

## Fan-out (FR-010, `004` FR-053)

Multi-shard/partition `Scan`/`Mutate`/`Aggregate`: one coordinator task per shard; quorum applied **per shard**. A shard that misses quorum:

- `partial_ok.value == false` (default) → whole query `Unavailable` or `QuorumUnachieved` naming the shard and level.
- `partial_ok.value == true` → rows from successful shards plus `PartUnavailable` warning; terminal `Done` with `partial = true`. The client MUST NOT be told the result is complete.

Empty successful shard → zero rows, **not** unavailability.

## Unavailability (FR-010, FR-011)

Zero live replicas for a required part → `part_unavailable{container, shard, live=0}`. Never a silent empty success. Partial only when `partial_ok` is on as above.

## Write path (FR-031–FR-033)

- Coordinator in a **log-follower** domain: forward write to source (`012`). `LOCAL_*` write still needs durable source acks.
- `LOCAL_*` read on follower MAY be stale local apply.
- Persistent/hybrid write acks: wait for `013` durability. Memory-mode: memory acks only for memory-mode containers (`004` FR-034).
- Leaderless in the source domain: no query-level write serializer.

## EACH_QUORUM (FR-032)

If the container’s async follower policy is default-refuse (`004` FR-074): plan fails with `each_quorum_async{group}` . Queued send ≠ ack.

## Ranking (FR-017)

Among candidates that already satisfy quorum, sort by `RankKey` ([data-model.md](../data-model.md) §8). Shuffle workers use the same function. Missing ladder value is not local. Cross-domain HLC ignored.

## Cancel (FR-013)

Session cancel token (disconnect or explicit) → abort tasks, delete spill, `Cancelled` within the timeout SLA window.

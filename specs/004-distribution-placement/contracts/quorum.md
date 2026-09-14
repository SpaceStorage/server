# Contract: Quorum Arithmetic and Durable Acknowledgements

**Feature**: `004-distribution-placement` | Crate: `crates/placement` (`quorum.rs`) | Spec: FR-028–FR-037 | Extends [`002` query-options](../../002-protocol-drivers/contracts/query-options.md)

## Vocabulary

Unchanged from `002`: `ONE`, `TWO`, `THREE`, `QUORUM`, `LOCAL_QUORUM`, `EACH_QUORUM`, `LOCAL_ONE`, `ALL`, `Acks(n)` (`1..=65535`). Case-insensitive in; upper-case out.

## Arithmetic

`n` = replica-set size of the **shard** (or container if unsharded). `local_n` = replicas in the requester's locality domain (`cluster.locality_key`, default `region`).

| Level | Required acks |
|-------|----------------|
| `ONE` | 1 |
| `LOCAL_ONE` | 1 in the local domain |
| `TWO` / `THREE` | 2 / 3 |
| `Acks(n)` | n |
| `QUORUM` | `floor(n/2)+1` |
| `LOCAL_QUORUM` | `floor(local_n/2)+1` |
| `EACH_QUORUM` | `LOCAL_QUORUM` in **every** destination group that holds replicas |
| `ALL` | n |

Worked `QUORUM`: RF 2→2, 3→2, 4→3, 5→3.

## Precedence

```text
quorum = query ?? session ?? container.capability.quorum.{write|read} ?? namespace_default (07) ?? global
```

This feature **adds** the container level. `002` resolvers gain a `container` slot; until a container is known (DDL), skip it.

## Clamp vs reject (`002` Q1, FR-033)

| Source | Unsatisfiable |
|--------|----------------|
| query / session | reject `QuorumUnsatisfiable{requested, available}` before execution |
| container / namespace / global | clamp to highest satisfiable; `clamped_from` recorded |

## Durable write acknowledgements (Clarification Q3, FR-034)

| Container mode | Counts toward **write** quorum |
|----------------|--------------------------------|
| persistent, hybrid | only `replica.durable == true` (drive-backed) |
| memory | memory acknowledgements |

Memory replicas of a persistent container may still be contacted (`ack_kind: memory`) but do not increment `achieved` for writes. If not enough durable acks arrive before timeout ⇒ fail, never success. Reads count any `in_sync` or `behind` replica the level permits; they do not use the durable filter.

## `EACH_QUORUM` and async groups (Clarification Q4, FR-074)

If any destination group is `async` and `each_quorum_policy` is `refuse` (default): `EACH_QUORUM` and `ALL` that would wait on that group ⇒ `QuorumRequiresAsyncGroup` **before** internodes wait. Policy `wait`: those requests treat the async group as sync for that call only. Queued send is never an ack.

## Read-your-write matrix (FR-036)

Documented in `docs/replication.md`. Guarantee: `QUORUM` write then `QUORUM` read on the same coordinator returns the acknowledged value. `ONE`/`LOCAL_ONE` reads may be stale under async replication; staleness bound is the replica's lag.

## Execution record

Extends `002` `AppliedOptions` with `coordinator`, `contacted[]`, `acks[]` (`node`, `ack_kind`), `achieved`, `durable_required`. Admin `executions` already exists; conformance asserts SC-005/006 from it.

## Conflicts (Clarification Q2, FR-037)

Default last-writer-wins on HLC `VersionStamp`. Disagreement on a read: return the greater stamp, record the conflict, repair stale replicas in the background. Type-supported `ConflictMerge` if declared. No client-held conflicts. Clock skew > `max_stamp_skew` ⇒ node `unhealthy_clock`.

# Contract: Source domain vs log-follower

**Feature**: `012-internode-and-time` | Crates: `placement`, `exec`, `internode`, `replication` | Spec: FR-006–FR-009, FR-011 | Extends [`004` quorum](../../004-distribution-placement/contracts/quorum.md)

## Mapping from `004` groups

| `004` destination group | This feature |
|-------------------------|--------------|
| `mode=sync` | replica targets in `container.source_domain` |
| `mode=async` | replica targets in a `follower_domain` |
| `cluster.locality_key` | **obsolete** for voting; use `quorum_domain`. Ladder remains farness |

`004` still chooses **which nodes** hold copies. This contract chooses **which of those copies count**.

## Write acknowledgements

| Level | Counts |
|-------|--------|
| `ONE` / `LOCAL_ONE` / `TWO` / `QUORUM` / `THREE` / `Acks(n)` | durable replicas in **source** only (`013`) |
| `EACH_QUORUM` | source quorum **plus** apply of that source-log position in each opted-in follower, or refused if `each_quorum_policy=refuse` |
| `ALL` | every declared replica including followers; subject to `004` FR-074 if followers are async |

`LOCAL_*` names the **coordinator’s** `quorum_domain`, not an AZ. `LOCAL_ONE` **write** = one durable WAL in the **source**, even if the coordinator is a follower.

## Paths

- Coordinator in source A: leaderless `FanoutWrite` to A replicas (`004` R19).
- Coordinator in follower B: `ForwardWrite` to A. B MUST NOT open a source log. B MUST NOT ack from a local WAL.
- Ordered/log types: forward to `006` leader in A, or `IndependentFollowerWrite`.

## Failure of source quorum

If A cannot meet the **requested** level → `QuorumUnsatisfiable` (no follower WAL).

Session/query option `quorum_fallback=LOCAL_ONE` (`002` options / `004` precedence **query then session** only; never container/global silent). If one durable source WAL is obtained → success; response `met=LOCAL_ONE`. If not → still fail. Undeclared fallback MUST NOT downgrade.

## Reads

- In A: requested level in A.
- `LOCAL_ONE` read in B: MAY serve applied local replica (stale OK).
- Stronger read in B: forward to A if B cannot satisfy.

## `multi_active`

Catalog default off. Ordered/log forced off. Create with `on` → `MultiActiveRefused` (first binary).

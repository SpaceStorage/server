# Contract: Hybrid logical clock

**Feature**: `012-internode-and-time` | Crate: `crates/clocks` | Spec: FR-012–FR-014, FR-018 | Used by internodes, placement, controlplane

## Stamp

```text
Hlc { domain, physical_micros, logical, node_id }
```

Tick: `physical = max(wall_micros, last.physical)`; if equal, `logical += 1`, else `logical = 0`. Persist last tick `{data_dir}/clocks/<domain>.json` before `ready`.

## Compare

Same `domain` only. Order: physical, logical, `node_id`. Different domain → `HlcCrossDomain` (callers MUST use source log position for followers).

## In-domain conflict

Default LWW: greater stamp wins. Type descriptor MAY supply `ConflictMerge` (deterministic). Record disagreement; repair stale replicas. MUST NOT wait for a client. Ordered/log types MUST NOT use LWW to merge two leadership histories (`006`).

## Skew

Heartbeat carries sender HLC. `|physical_remote - physical_local| > replication.max_stamp_skew` (default **500ms**) in the same domain → `node_state=degraded` (`unhealthy_clock`). Writes continue. Cross-domain skew is not this signal (not compared).

## Planner

Internodes RTT EMA and HLC skew samples are published for `004`/`005` ladder-rank override once samples exist.

# Contract: Internode / clock / replication metrics

**Feature**: `012-internode-and-time` | Crates: `internode`, `replication`, `clocks` | Exposition owned by `008`

This crate **increments** these names. It MUST NOT rename or drop series required by `08`. Replication family in [`008` catalog](../../008-observability/contracts/catalog.md) is shared with `004` (lag/queue/throughput). Additive fabric series:

| Name | Type | Labels | Notes |
|------|------|--------|-------|
| `spacestorage_internode_peers` | gauge | `node` | connected internodes peers |
| `spacestorage_internode_heartbeat_rtt_seconds` | histogram | `node`, `peer` | RTT samples |
| `spacestorage_failure_detector_unavailable` | gauge | `node`, `peer` | 1 if FD-unavailable |
| `spacestorage_hlc_skew_seconds` | gauge | `node`, `quorum_domain`, `peer` | physical delta |
| `spacestorage_hlc_skew_over_limit` | gauge | `node`, `quorum_domain` | 1 if over `max_stamp_skew` |
| `spacestorage_quorum_forward_total` | counter | `node`, `result` | `ok`, `unsatisfiable`, `fallback_local_one` |
| `spacestorage_promote_total` | counter | `result` | `ok`, `not_caught_up`, `needs_accept`, `two_sources` |
| `spacestorage_epoch_fenced_total` | counter | `container` | old-epoch writes refused |

Also increment `08` `spacestorage_replication_*` on source-log and stream activity, and set `node_state=degraded` when skew over limit (existing `001`/`008` `node_state`).

`result` values are closed. Omit label keys when unknown (`008` FR-023).

# Contract: Replication Modes, Destination Groups, Stamps

**Feature**: `004-distribution-placement` | Crate: `crates/placement` (`group.rs`, `stamp.rs`) | Spec: FR-038–FR-044, FR-071–FR-075

## Destination groups

A replication declaration expands to ≥1 group (research R8).

```text
capability.replication.factor=3
capability.replication.anti_affinity=az
capability.replication.mode=sync          # default; one group over the whole topology
```

Multi-group (flat options, indexed):

```text
capability.replication.group.0.selector=region=eu
capability.replication.group.0.factor=3
capability.replication.group.0.mode=sync
capability.replication.group.1.selector=region=us
capability.replication.group.1.factor=2
capability.replication.group.1.mode=async
capability.replication.group.1.lag_threshold=60s
capability.replication.each_quorum_policy=refuse
```

Per-label-value counts (FR-023) are groups whose selector is `key=value`.

## Sync vs async (FR-039, FR-040)

- **sync**: coordinator waits until write quorum is met by **counted** acks from that group (durable filter applies).
- **async**: coordinator does not wait; ships the write after acknowledging the client. Correctness for readers is a function of read quorum (document the level pair). Lag is required (FR-041): `{ duration, pending_writes }`, threshold, health `healthy|unhealthy`. A read served from a lagging replica may include `staleness: <lag>`.

Local write at `LOCAL_QUORUM` MUST NOT include an async remote RTT (FR-072, SC-010).

## Unreachable sync destination (FR-043)

Container declares `sync_unreachable: fail | demote_async`. Default `fail`. In effect is in the description; the cluster never chooses silently.

## Version stamps

HLC `(physical_micros, logical, node_id)` assigned by the coordinator at fan-out. Compare physical → logical → node_id. LWW default; type descriptor may supply `ConflictMerge`. Replication never bypasses the abstract datatype interface (FR-044): internodes payload is a `CanonicalValue` plus stamp, applied through `Container` ops on the receiving node.

## Catch-up (FR-042)

Hints while `now - unavailable_since < hinted_handoff_window`. Else full compare. Switch is reported on the replica (`repair.kind: compare`).

## Timing knobs

All per cluster and overridable per group (FR-073). Defaults in research R17. The minutes-RTT starter sets `failure_timeout 30m`, `hinted_handoff_window 168h`, `txn_timeout 1h`.

## Starter topologies (FR-075)

Fixtures under `fixtures/`: single node, single rack, three AZ, two regions (sync local / async remote), mixed media, long-RTT (same as two-region with timing overrides). Each must pass `spacestorage validate` and produce the placement the [quickstart](../quickstart.md) states.

# Contract: Tombstones, `gc_grace`, TTL

**Feature**: `013-durability-and-recovery` | Crate: `crates/storage` | Spec: FR-009, FR-010, FR-012

## Delete visibility

A delete is visible to a subsequent read at the requested quorum once enough replicas have applied the tombstone (`012` who counts).

## Drop rule

Compaction MAY drop a tombstone only if **all** hold:

1. Every replica in the source `quorum_domain` has seen it, **or** was rebuilt from a snapshot newer than the delete.
2. `gc_grace` elapsed (default **24h**, namespace/container override).
3. Async remotes: source MUST NOT drop a tombstone still required by a follower not caught up on the source log, unless that remote rebuilds from snapshot (`012`).

## TTL

If the type has an event-time field, expiration uses it; otherwise ingest/HLC in the **source domain**. Expiry creates a tombstone via job `TTL expiration`. Silent omit is forbidden.

TTL event-time is **not** the PITR clock ([pitr.md](pitr.md)).

## Three states

Null, missing field, and tombstone stay distinct (`003` value domains). Compaction MUST NOT collapse them except by the drop rule above.

# Contract: Sharding and Partitioning

**Feature**: `004-distribution-placement` | Crate: `crates/placement` (`shard.rs`) | Spec: FR-050–FR-056

## Sharding

Declared: `capability.sharding.key=<field>` (fixed after create, `003`) and optional `capability.sharding.shards=N` (default 16).

Scheme: **rendezvous hashing** (HRW) over `N` shard ids using the canonical encoding of the key. Each shard is a `ReplicaSet` placed independently under the container's RF, anti-affinity, media and groups.

Routing: any coordinator computes the same shard for a key (FR-052). Write quorum applies **inside** that shard (FR-053).

Reshard (`shards` change): add/remove shard ids, move affected keys in the background. During the move a key still maps to exactly one shard (dual-write window is forbidden; use a forwarding cursor on the old shard until the new one is `in_sync`, then flip the map atomically in the placement log). No acknowledged write lost or duplicated (FR-054).

Imbalance: shard bytes or request-rate **> 2× median** ⇒ rebalance candidate (FR-056). No silent re-key (that's `10`).

## Partitioning

`capability.partitioning.scheme=range|time` plus `key` / `interval`. Hash partitioning is **not** a partitioning scheme (it is sharding); a conflict is `CapabilityConflict` from `003`.

- **range**: splits `[lo, hi)` → replica set. Auto-split when a partition exceeds `partition_split_bytes` (default 8 GiB). Operator split via admin.
- **time**: buckets of `interval` on a timestamp field. First write in a bucket creates the partition. `partition_policy` may override selector per age (recent → NVMe, old → HDD) (Story 6 scenario 5).

Still one container to clients.

## Multi-shard queries

Coordinator fans out, applies quorum per shard, combines. A shard that misses quorum fails the **whole** query with the shard id and level, unless the client set `partial=true` on `QueryOptions` (new optional field, default false) (FR-053, SC-012). Partial answers are never labelled complete.

## Invalid declarations

Type does not support sharding/partitioning, key absent from schema, partitioning key not contained in sharding key when the type requires it: refused by `003` before placement (FR-055). This feature never receives them.

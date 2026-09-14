# Contract: Topology (nodes, labels, drives, memory)

**Feature**: `004-distribution-placement` | Crate: `crates/placement` (`topology.rs`) | Spec: FR-001–FR-010

## Labels

Operator-defined key/value pairs. Keys are open-ended IDENT. Starter keys documented: `rack`, `az`, `region`. Deeper keys (`dc`, `continent`, `planet`) have identical semantics.

Two maps per node, always distinguished in the view:

- `labels` — declared
- `derived_labels` — from drives (`media_<kind>=true`) and from the memory pool (`memory=true`)

A label change on a live node is a `PlacementEvent::LabelChange`, does not restart the process, and re-evaluates every placement that references the key (FR-009).

## Drives

Repeatable `storage { drive NAME { path; media; size?; labels{} } }`. Media vocabulary: `nvme`, `ssd`, `hdd`, plus any IDENT (`other`). Expandable without migrating existing declarations. Capacity from `size` or filesystem. Free space is a placement input (FR-010); a node whose matching drives are full is excluded with `no_media_capacity`.

Drive properties enrich node `derived_labels` so `media=nvme` in a selector matches the node, then the planner pins the replica to a specific drive.

## Memory pool

`memory { size S; labels { } }` already parsed by `003`. This feature treats absence as "no pool" (FR-005): memory-mode placements exclude the node with reason `no_memory_pool`. Size must be present when the block exists (`memory_size_required`). A pool larger than reported RAM is refused (`memory_exceeds_machine`).

## Topology view

Readable from any node (`spacestorage topology`, admin `topology`):

```json
{
  "locality_key": "region",
  "nodes": [
    {
      "name": "db-1",
      "status": "live",
      "labels": { "rack": "r1", "az": "az1", "region": "eu" },
      "derived_labels": { "media_nvme": "true", "memory": "true" },
      "drives": [{ "name": "nvme0", "media": "nvme", "capacity_bytes": 0, "used_bytes": 0, "labels": {} }],
      "memory": { "size_bytes": 8589934592, "used_bytes": 0, "labels": { "tier": "ram" } }
    }
  ],
  "domains": { "rack": 3, "az": 3, "region": 1 }
}
```

`domains.<key>` is the cardinality of distinct values (FR-007). Two nodes `az1/region1` and `az2/region1` ⇒ `az: 2`, `region: 1`.

The view is identical on every live node once `CatalogDelta` has been applied (FR-006). Stale view: coordinator refuses with `PlacementViewStale` rather than guessing (FR-047).

## Validation

See [data-model.md](../data-model.md) codes. Duplicate label keys, missing media, missing memory size, colliding node names: refused as a whole, never half-applied (FR-008).

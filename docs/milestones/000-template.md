# Milestone: \<slug\>

**Profile**: `first-binary` | `complete-product`  
**Tag**: prefer `slices-1-5` over marketing `v1.0.0` for the first binary

## Implemented

List slice ids `1..=k` (prefix, no holes).

## Deferred

List every slice `id > k` that remains owed. Each entry MUST stay in intent;
`still_owed: true` in the YAML sibling. Example:

- Slice 6 — remaining `015` handlers
- Slice 7 — Raft, quotas, full authz
- Slice 8 — query beyond CRUD
- Slice 9 — full `008` catalog
- Slice 10 — migration and PITR
- Slice 11 — UIs and ingest

## Changelog

Describe operator-visible changes. Do not call the first binary a “v1” of the
seven-protocol matrix.

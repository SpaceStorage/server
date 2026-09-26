# Milestone: control-plane-tenancy-authz (slice 7)

**Profile**: `control-plane-tenancy-authz`  
**Tag**: `slices-1-7`

## Implemented

Slices `1..=7` shipped. Ledger: [003-control-plane-tenancy-authz.yaml](003-control-plane-tenancy-authz.yaml).

Slice **7** DoD:

- Raft control plane (`006`): `crates/controlplane` with cluster + namespace groups, odd voter sets, static 1→3 expansion, lightweight Raft log/store (fsync via `spawn_blocking`), `controlplane-ops` for Replace/Grow/Shrink + `controller_exclusive_data`
- Config: `cluster.raft { heartbeat; election_timeout; }` validation; exclusive-data gated without ops
- Internode additive Raft RPC message types
- Membership: `apply_controlplane_roster` beyond seed-roster-only pull
- Tenancy (`007`): `crates/tenancy` registry, quotas/usage/admit behind `tenancy-quotas`, builtin roles, encryption declaration
- Authz (`014` full vocabulary): closed verb set, implicit tenant grant, `authz-custom` RolePut gate, Authorizer effective grants
- Release profile `control-plane-tenancy-authz` (k=7) in `spacestorage-release-profile`

## Deferred

Still owed (`still_owed: true`):

- Slice 8 — query beyond CRUD
- Slice 9 — full `008` catalog
- Slice 10 — migration and PITR
- Slice 11 — UIs and ingest

Do **not** brand this milestone as `complete-product`.

## Changelog

- Added `crates/controlplane` (`spacestorage-controlplane`)
- Added `crates/tenancy` (`spacestorage-tenancy`)
- Deepened `crates/authz` custom-role / implicit-grant surface
- Extended config raft + exclusive-data directives
- Extended internode registry with Raft/quota message ids
- Milestone YAML claims `implemented: [1..7]` with deferred 8–11

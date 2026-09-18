# Contract: Membership store (`ClusterStore` events)

**Feature**: `011-identity-membership` | Crate: `membership` writes; `controlplane` applies | Spec: FR-009, FR-013 | Seam: [`006` cluster store](../../006-control-plane/contracts/cluster-store.md)

## Source of truth

`snapshot().members` is the membership view. `snapshot().pending` is admin-only and **must not** be treated as members by `004` placement or `006` voter sets.

`006` `MemberRecord.status` values used here: `ready`, `draining`. Do **not** store pending as `joining`.

## Commit rule

Every event in [data-model.md §10](../data-model.md) is cluster-scoped. Success = majority of **cluster voters**. Minority → `Minority { group: cluster }`. Admit/token/decommission/replace attempted on a partitioned minority MUST NOT add or remove a member.

## Bootstrap

Explicit `cluster { bootstrap; }`: if `identity/cluster.json` already has a UUID, reuse it (do not create a second cluster). If disk has a UUID and config asks `join`, follow persisted membership (FR-016). Two processes that both bootstrap with empty seeds get two UUIDs (SC-002).

## Apply hooks

| Event | `006` | `004` |
|-------|-------|-------|
| `AdmitMember` | insert member; maybe learner/voter per R12 | not a new replica target until admitted (then eligible) |
| `Drain` | status draining | exclude from **new** placements |
| `Undrain` | status ready | eligible again |
| `RemoveMember` | drop member; voter-set adjust (R12) | rebalance must already have completed or data-loss accepted |
| `ReplaceMember` | incarnation++; keep placements | no rewrite of replica locations |
| `RetireIdentity` | add to retired set | n/a |

Join by itself MUST NOT move existing replicas (spec US2).

# Data Model: Cluster Identity, Discovery, Join, Leave, and Replace

**Feature**: `011-identity-membership` | **Date**: 2026-09-18 | **Spec**: [spec.md](spec.md)

Applied cluster membership is stored by `006` / `004` `ClusterStore`. This document is the procedure-facing model. Validation codes are in contracts.

## 1. NodeIdentity (local `node.json`)

| Field | Type | Notes |
|-------|------|--------|
| node_id | UUID | Random on first start; never changes |
| node_name | string | From `node { name; }`; unique among **current** members |

**Invariants**: independent of hostname, IP, PID. After decommission, `node_id` is retired cluster-wide; a new process MUST generate a new id unless this is **replace**.

## 2. ClusterIdentity (local `cluster.json` + cluster log)

| Field | Type | Notes |
|-------|------|--------|
| cluster_uuid | UUID | Created at bootstrap; immutable |
| cluster_name | string | Human label; not unique in the universe |
| secret_epochs | `[SecretEpoch]` | Currently accepted join secrets |

Two bootstraps with the same `cluster_name` produce two UUIDs. No merge protocol.

## 3. SecretEpoch

| Field | Type |
|-------|------|
| epoch | u64, monotonic |
| secret | opaque bytes (file + wrapped copy in identity dir) |
| accepted | bool |

**Invariants**: internodes accept any `accepted=true` epoch (overlap). `secret-rotate complete` or `max_overlap` sets the previous epoch `accepted=false`. Verify is constant-time over accepted secrets.

## 4. PendingJoin (cluster log, **not** in `members`)

| Field | Type |
|-------|------|
| node_id | UUID |
| node_name | string |
| labels | map (must include every ladder key) |
| internodes_address | string |
| requested_at | HLC |
| token_id | optional UUID |

**Invariants**: not a voter, not a replica target, not `ready` as a member. Duplicate pending `node_id` or current-member `node_name` → refuse. Process exit before admit: row MAY remain until TTL (default = token default 12 h) then dropped; a later start is a new first join.

## 5. JoinToken

| Field | Type |
|-------|------|
| token_id | UUID |
| node_name | string |
| node_id | optional UUID |
| expires_at | wall + HLC bound |
| used | bool |
| minted_by | principal id |

**Invariants**: single use; hours-scale TTL (default 12 h, 1–72 h). Used or expired → refuse + audit. Successful token join = `AdmitMember` without a second admit.

## 6. MemberRecord (cluster log `members` map)

Aligns with `006` data-model, with `joining` removed:

| Field | Type |
|-------|------|
| node_id | UUID |
| node_name | unique among current members |
| addresses | internodes / replication / tenant |
| labels | topology ladder keys + others |
| status | `ready` \| `draining` |
| incarnation | u64, starts at 1; replace increments |
| voter | bool (derived from `006` VoterSet, not duplicated as source of truth) |

Unavailable-for-quorum is **not** a membership status; it is `012` failure-detector state. Replace reads that state.

**Invariants**: `node_name` unique in `members`. Pending names that collide with `members` are refused. After decommission the name MAY reappear on a **new** `node_id`.

## 7. RetiredIdentity

| Field | Type |
|-------|------|
| node_id | UUID |
| retired_at | HLC |
| former_name | string (informational) |

**Invariants**: first join with this `node_id` → `RetiredIdentity`. Replace is **not** allowed after retire (replace requires the id still in `members` and FD-unavailable).

## 8. MembershipView

| Field | Type |
|-------|------|
| cluster_uuid | UUID |
| cluster_name | string |
| members | map `node_id → MemberRecord` |
| pending | map `node_id → PendingJoin` (admin-visible; not a member list) |
| retired | set of node_id |
| secret_epoch | u64 (current primary epoch) |

Every live **member** converges on the same `members` map (`006` majority). Pending is cluster-visible so any CLUSTER_ADMIN can admit.

## 9. State transitions

```text
(not in view, no seeds, not bootstrap) → refuse ready

bootstrap → MemberRecord(ready, incarnation=1), voter set {self}

first join + secret + ladder
  ├─ no token → PendingJoin
  │                 ├─ Admit → MemberRecord(ready)
  │                 └─ exit / expire → gone (not a member)
  └─ valid token → MemberRecord(ready)  # no pending wait

Member ready → Drain → draining (process up)
draining → Undrain → ready
draining → Stop (`001`) → process exit; member still draining until restart or decommission
draining + Decommission → 004 rebalance → RemoveMember + RetiredIdentity

Member FD-unavailable + Replace(same node_id) → incarnation++ , FenceIncarnation
Member heartbeating + Replace → LiveReplace (refused)
```

## 10. ClusterStore events (this feature writes)

| Event | Effect |
|-------|--------|
| `Bootstrap { uuid, name, node }` | create cluster + first member |
| `PendingJoin { … }` | insert pending |
| `AdmitMember { node_id, token_id? }` | pending→member or token path |
| `MemberUpdate { node_id, addresses?, labels? }` | identity unchanged |
| `Drain { node_id }` / `Undrain { node_id }` | status |
| `RemoveMember { node_id }` + `RetireIdentity { node_id }` | decommission |
| `ReplaceMember { node_id, incarnation }` | fence old process |
| `SecretRotateBegin { epoch }` / `SecretRotateComplete { epoch }` | overlap |
| `JoinTokenMint` / `JoinTokenConsume` | token table |

Routing: all of the above are **cluster-scoped** (`006` cluster group). Minority → `Minority { group: cluster }` (no membership change).

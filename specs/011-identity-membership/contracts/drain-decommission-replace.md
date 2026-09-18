# Contract: Drain, decommission, and replace

**Feature**: `011-identity-membership` | Crates: `membership`, `node`, `placement` | Spec: FR-010–FR-012, FR-015–FR-018

## Operator drain / undrain

| Op | Process | Membership | Tenant accept | New placements |
|----|---------|------------|---------------|----------------|
| `Drain` | stays up | `draining` | no new | no |
| `Undrain` | stays up | `ready` | yes | yes |
| `Stop` (`001`) | exits after drain timeout | still a member until restart/decommission | no | no |

Existing replicas remain until rebalance or decommission. In-flight tenant requests use the `001` drain timeout **only on Stop**. Operator drain finishes in-flight requests then keeps listening on internodes/admin.

## Decommission

`Decommission { node_id, accept_data_loss }`:

1. If the process is reachable, it MUST be `draining` (call `Drain` first if `ready`).
2. Ask `004` to re-place replicas that live here onto members that satisfy container constraints.
3. If blockers remain and `accept_data_loss` is false → `DecommissionBlocked { containers[] }`; process stays up.
4. Else `RemoveMember` + `RetireIdentity`. Voter-set adjust per research R12.
5. Name is reusable; identity is not.

Gone process: skip copy-from-self; re-place from remaining replicas or require `accept_data_loss`.

## Replace

Allowed when `012` marks `node_id` unavailable (heartbeat timeout elapsed) **now**. No extra ping. Heartbeating `ready`/`draining` → `live_replace`.

On success: `incarnation += 1`; `FenceIncarnation`; placements keep the id. Authorization: CLUSTER_ADMIN (first binary: admin bearer). The new process presents the **same** `node_id` in `JoinRequest.replace_of`.

A first join presenting a **retired** id is not replace → `retired_identity`.

## Rolling restart

Stop one member (`001`); start it; wait `ready` **and** replica catch-up (`004`/`013`); then the next. No new admit/token (FR-015/016).

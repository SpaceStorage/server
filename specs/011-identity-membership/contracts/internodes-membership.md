# Contract: Membership on `internode`

**Feature**: `011-identity-membership` | Crates: `membership`, `internode` | Spec: FR-006–FR-009, FR-012

Frame, TLS/plaintext, and replication-role auth: [`012`](../../012-internode-and-time/spec.md) and [`004` coordinator](../../004-distribution-placement/contracts/coordinator.md). No new port. Join secret (`cluster.token_file`) MUST verify before these types are processed.

## Message types (additive)

| Type | Direction | Purpose |
|------|-----------|---------|
| `JoinRequest` | candidate → seed/member | node_id, node_name, labels, addresses, optional token, optional `replace_of` |
| `JoinAck` | member → candidate | `pending` \| `admitted` \| `replaced` \| `refused` + code |
| `PendingAnnounce` | leader → members | admin-visible pending row (also in cluster log) |
| `FenceIncarnation` | members | `{ node_id, min_incarnation }` after replace |
| `SecretRotate` | leader → members | `{ epoch, phase: begin\|complete }` |

Unknown types remain ignored without tearing the mesh (`004`).

## Rules

- `JoinRequest` is the only cluster message a **non-member** may send (after secret verify).
- Ladder omit / integrity failure → `JoinAck.refused` code `ladder`; **no** pending row.
- Name in current `members` → `name_in_use` (unless replace of that id).
- Retired `node_id` on a first join → `retired_identity`.
- Replace path: `replace_of` set, FD-unavailable, CLUSTER_ADMIN already recorded or token bound to that id; else `live_replace` / `not_replace_eligible`.
- Stale incarnation on any later internodes/replication frame → close; do not vote.
- Backpressure: slow/fail the stream; do not block Tokio workers (`012`).

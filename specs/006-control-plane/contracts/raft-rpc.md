# Contract: Raft on `internode`

**Feature**: `006-control-plane` | Crates: `controlplane`, `internode` | Spec: FR-014

Frame, TLS/plaintext, and replication-role auth: [004 coordinator](../../004-distribution-placement/contracts/coordinator.md) and `012`. No new port.

## Message types (additive)

| Type | Direction | Purpose |
|------|-----------|---------|
| `RaftVote` | voter ↔ voter | RequestVote / response (`openraft`) |
| `RaftAppend` | leader → follower/learner | AppendEntries |
| `RaftSnapshot` | leader → follower/learner | InstallSnapshot chunks |
| `RaftForward` | any → leader | Client metadata write that landed on a non-leader |
| `MetricsPush` | member → namespace primary | Local shared-datatype figures |

Unknown types remain `unknown_message` without tearing the mesh (`004`).

Payload: length-prefixed **binary** (bincode or similar, documented in crate) for Raft bodies; JSON allowed for `RaftForward` admin-originated ops. Version byte already in the internodes frame.

## Rules

- Only **voters** of `group` process `RaftVote`. Learners process append/snapshot, never vote.
- Non-members: close or `NotMember`; do not vote (`FR-010`).
- Backpressure: slow/fail the stream; do not block Tokio workers (`012`).
- Mixed-version N/N+1: internodes version window is `015`; Raft log **format version** refuse is local start (`013`).

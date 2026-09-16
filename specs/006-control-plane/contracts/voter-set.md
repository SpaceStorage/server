# Contract: Controller voter set

**Feature**: `006-control-plane` | Spec: FR-002, FR-010, FR-018, FR-020 | Model: [data-model.md](../data-model.md)

## Odd size

Accepted committed voter sets have size 1, 3, 5, … Proposed even size → `VoterSetOdd`.

## First binary (static)

| Members admitted | Cluster (and new-namespace) voters |
|------------------|--------------------------------------|
| 1 | `{A}` |
| 2 | `{A}` (B learner) |
| 3 | `{A,B,C}` in **one** config change |
| 4+ | still `{A,B,C}`; extras learners |

No replace/grow/shrink API. Calling them → `Slice7Required`.

## Slice 7 (`controlplane-ops`)

| Op | Accepted step |
|----|----------------|
| Replace `from` → `to` | One change; size unchanged; `to` must be a member; `from` remains a member (non-voter) unless also decommissioned (`011`) |
| Grow | Add **two** members as voters |
| Shrink | Remove **two** voters, leftover odd and ≥1 |

Any step that cannot keep a majority during joint consensus → `VoterSetMajorityLost`; previous set remains.

## Membership vs voting

`011` records membership first. Then this crate may add the node as learner; voters only via the table above. Non-members never appear in `VoterSet`.

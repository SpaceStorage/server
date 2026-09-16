# Contract: Admin API and CLI

**Feature**: `006-control-plane` | Crates: `admin-proto`, `spacestorage`, `node` | Extends `001` CLI

## Admin ops (JSON, both handlers)

| Op | Result |
|----|--------|
| `Controllers` | `[ControllerView]` (cluster + each namespace) |
| `RaftStatus { group }` | term, commit, role, voter/learner lists |
| `VoterReplace { group, from, to }` | slice 7; else `Slice7Required` |
| `VoterGrow { group, add: [id, id] }` / `VoterShrink { group, remove: [id, id] }` | slice 7 |
| `Leases { namespace }` | `[]` in first binary unless an ordered type exists |

Parity over `admin` and `admin-http` (`001` FR-020).

## CLI

| Command | Notes |
|---------|--------|
| `spacestorage controllers` | table: group, primary, secondaries, commit |
| `spacestorage raft-status [--group cluster\|<ns>]` | |
| `spacestorage controllers voters replace --group … --from … --to …` | slice 7 |
| `spacestorage executions` | unchanged; data path still any member |

`--output json` supported. Exit codes: `001` plus 3 = `NotLeader`/`Minority` (retryable).

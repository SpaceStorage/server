# Contract: Election metrics this crate emits

**Feature**: `006-control-plane` | Spec: FR-016 | Catalog: `008` FR-009

Figures only; exposition is `08`. Do not rename.

| Intent name | Prometheus name (reserved) | Type | Labels |
|-------------|----------------------------|------|--------|
| leader elections total | `spacestorage_leader_elections_total` | counter | `node`, `raft_group`, `result=success\|error` |
| leader elections errors total | included as `result=error` | | |
| histogram of leader election duration | `spacestorage_leader_election_duration_seconds` | histogram | `node`, `raft_group` |

`raft_group` is `cluster` or the namespace name. Required `08` labels that do not apply are omitted (not invented).

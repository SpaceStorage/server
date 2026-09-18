# Contract: Admin / CLI

**Feature**: `012-internode-and-time` | Crates: `admin-proto`, `spacestorage` | Spec: FR-005, FR-010, FR-014 | Auth: `CLUSTER_ADMIN` (first binary: `001` admin bearer, same as `011`)

| Verb | Args | Effect |
|------|------|--------|
| `domain-create` | `NAME` | Empty `QuorumDomain` |
| `domains` | | List names and members |
| `promote` | `CONTAINER --to DOMAIN` | Ordinary promote |
| `promote` | `CONTAINER --to DOMAIN --force --accept-data-loss` | Force promote |
| `clocks` | | Per-domain HLC, skew samples, `max_stamp_skew` |
| `fabric` | | Internodes/replication listen, peer FD status, RTT |

Errors: [quorum-domain.md](quorum-domain.md), [promote.md](promote.md). `spacestorage health` already includes `node_state`; skew degraded must appear there.

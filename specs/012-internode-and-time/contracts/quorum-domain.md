# Contract: Quorum domain

**Feature**: `012-internode-and-time` | Crates: `internode`, `controlplane`, `membership` | Spec: FR-005, FR-006 | Model: [data-model.md](../data-model.md)

## Bootstrap

First node (`011` bootstrap) creates `QuorumDomain { name: default, members: [self] }`.

## Create

```text
spacestorage domain-create <name>
```

`CLUSTER_ADMIN`. Name unique. Empty membership allowed. Ladder keys (`region`, `planet`, …) are **not** created as domains.

## Join

`JoinRequest.quorum_domain` required (`011` additive field). Must name an existing domain. Omit → `JoinAck.refused` code `quorum_domain_required`. Unknown → `quorum_domain_unknown`. Node already in another domain (replace into a different domain) → `quorum_domain_change` (replace keeps identity **and** domain; to move, decommission + join).

## Cardinality

A member is in exactly one domain. Live `domain-assign` → `quorum_domain_live_change`. Move = decommission or replace, then join naming the new domain.

## Who votes

Write-quorum set for a container = replica targets (`004`) whose `MemberRecord.quorum_domain == container.source_domain`. Followers are replica targets that do not count for `ONE`/`LOCAL_ONE`/`TWO`/`QUORUM` writes.

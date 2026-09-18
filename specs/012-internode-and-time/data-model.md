# Data Model: Internode Fabric, Clocks, Quorum Domain, and Conflict Resolution

**Feature**: `012-internode-and-time` | **Date**: 2026-09-18 | **Spec**: [spec.md](spec.md)

Cluster-scoped objects live in `ClusterStore` (`004`/`006`). Process-local clocks live under `{data_dir}/clocks/`. Tenant bytes stay in `003` storage. Validation codes are in contracts.

## 1. QuorumDomain (cluster object)

| Field | Type | Notes |
|-------|------|--------|
| name | string | Unique in the cluster. Bootstrap creates `default` |
| members | `[node_id]` | Every id is a current member (`011`) in **this** domain only |

**Invariants**: not a topology-ladder key. Labels MUST NOT silently create a domain. A node_id appears in **exactly one** domain. Live reassignment refused. Empty domain (no members) MAY exist after decommission so a later join can name it. Delete of `default` refused while it is the only domain.

## 2. NodeDomainBinding

Derived: `member.node_id → quorum_domain.name`. Stored on `011` `MemberRecord.quorum_domain` (new required field). Join payload must match an existing `QuorumDomain.name`.

**Invariants**: omit/unknown name → join refused, no pending row. Change requires decommission/replace + join, not a live edit.

## 3. HlcStamp

| Field | Type |
|-------|------|
| domain_id | domain name |
| physical_micros | u64 |
| logical | u32 |
| node_id | UUID |

**Compare**: only if `domain_id` equal; then physical → logical → node_id. Cross-domain compare is invalid (followers use source log position instead).

## 4. DomainClock (process-local)

| Field | Type |
|-------|------|
| domain | name (the one domain this node is in) |
| last | HlcStamp |
| persisted_at | path `{data_dir}/clocks/<domain>.json` |

**Invariants**: tick never decreases `(physical, logical)` for this node. Restart loads last. Skew samples vs peers in the same domain; over `max_stamp_skew` → `node_state=degraded`.

## 5. ContainerReplication (cluster / namespace catalog)

| Field | Type | Notes |
|-------|------|--------|
| container_id | UUID | |
| source_domain | domain name | exactly one |
| follower_domains | `[name]` | async log-followers; may be empty |
| epoch | u64 | starts at 1; promote increments |
| multi_active | bool | default false; create true refused in first binary |
| each_quorum_policy | `refuse` \| `wait` | owned with `004` |

**Invariants**: ordered/log types force `multi_active=false`. Two source domains for one container refused. Replica targets (`004`) in `source_domain` are the write-quorum set; targets in follower domains do not count for `ONE`/`LOCAL_ONE`/`TWO`/`QUORUM` writes.

## 6. SourceLogPosition

| Field | Type |
|-------|------|
| container_id | UUID |
| epoch | u64 |
| position | u64 | monotonic in this epoch, assigned in the source |

Followers store `applied_position` (and MAY store a durable copy of the record). Apply is in order. Last **source position** wins on followers; local HLC on a follower MUST NOT win.

## 7. FailureDetectorView (process-local, cluster-wide timeout)

| Field | Type |
|-------|------|
| peer | node_id |
| last_heartbeat | Instant |
| status | `alive` \| `unavailable` |

**Invariants**: `unavailable` iff `now - last_heartbeat >= cluster.failure_timeout` (default 15 s). Same status for quorum counting and `011` replace eligibility. Not a membership status (`ready`/`draining` unchanged). Heartbeat again → `alive` and counts.

## 8. FabricEntrypoint

| Field | Type |
|-------|------|
| handler | `internode` \| `replication` |
| address | IP; default `127.0.0.1` |
| port | u16; starters 7000 / 7001 |
| transport | `tls` \| `plaintext` |

**Invariants**: both handlers present. No shared port with admin/tenant. Loopback listen ⇒ cannot join remotes.

## 9. WriteAttempt (runtime)

| Field | Type |
|-------|------|
| coordinator_domain | name |
| requested_level | quorum vocabulary (`004`) |
| fallback | optional `LOCAL_ONE` |
| source_acks | durable count in source |
| outcome | `ok{met}` \| `failed` |

**Invariants**: follower coordinator never increments acks from local WAL. Fallback undeclared ⇒ no silent downgrade.

## 10. PromoteRequest

| Field | Type |
|-------|------|
| container_id | UUID |
| to_domain | follower domain |
| force | bool |
| accept_data_loss | bool |

**State transitions** for a container:

```text
source=A, epoch=n
  ordinary promote to B (B caught up OR A FD-unavailable)
    → source=B, epoch=n+1, A fenced
  force promote to B (accept_data_loss)
    → source=B, epoch=n+1, A fenced
  ordinary promote to B while A alive and B lagging
    → refused (no transition)
  force without accept_data_loss
    → refused
  old A write at epoch n after fence
    → refused
  A rejoins as follower
    → replica in follower_domains, applies source log at epoch n+1
```

## 11. Relationships

```text
Cluster 1──* QuorumDomain 1──* MemberRecord (exactly one domain each)
ContainerReplication.source_domain → QuorumDomain
ContainerReplication.follower_domains → QuorumDomain
HlcStamp.domain_id → QuorumDomain
SourceLogPosition per ContainerReplication.epoch
FailureDetectorView per peer, consumes heartbeat on internodes
FabricEntrypoint ×2 per node (internode, replication)
```

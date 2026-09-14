# Quickstart: Distribution, Placement, Media, and Replication

**Feature**: `004-distribution-placement` | **Plan**: [plan.md](plan.md) | Validates every success criterion of [spec.md](spec.md)

Walkthrough on a three-AZ loopback cluster, then mixed media, failure, shards, two-region async, and rebalance. CLI from `001`; every inspect step is also an admin op.

## Prerequisites

```bash
cargo build --release
# admin.token, users, cluster.token (0600), keyring as in feature 003
spacestorage validate --config contracts/fixtures/cluster-3az/db-1.conf
spacestorage validate --config contracts/fixtures/cluster-3az/db-2.conf
spacestorage validate --config contracts/fixtures/cluster-3az/db-3.conf
```

Expected: three `ok` results. Invalid fixtures under `fixtures/invalid/` each fail with the code named by the filename.

Start the three nodes (distinct `data_dir` already in the fixtures):

```bash
spacestoraged --config contracts/fixtures/cluster-3az/db-1.conf &
spacestoraged --config contracts/fixtures/cluster-3az/db-2.conf &
spacestoraged --config contracts/fixtures/cluster-3az/db-3.conf &
spacestorage --admin 127.0.0.1:7700 status   # ready
spacestorage --admin 127.0.0.1:7710 status
spacestorage --admin 127.0.0.1:7720 status
```

## 1. Topology view — SC-001, SC-002

```bash
spacestorage --admin 127.0.0.1:7700 topology
```

Expected within 30 minutes of following the starter (SC-001): three `live` nodes, labels `az=az1|az2|az3`, `region=eu`, `domains.az=3`, `domains.region=1`. The same view from ports 7710 and 7720 is identical. Single-node, rack, mixed-media and two-region starters in `fixtures/` validate and describe as their contracts state (SC-002).

## 2. Replicate with anti-affinity — SC-003

```bash
spacestorage --admin 127.0.0.1:7700 create tenant-a.orders \
  type=kv_store \
  capability.replication.factor=3 \
  capability.replication.anti_affinity=az

spacestorage --admin 127.0.0.1:7700 placement tenant-a.orders
```

Expected: three replicas on three AZs; report lists nodes and satisfied constraint. Repeat from db-2: same placement (FR-018).

```bash
spacestorage create tenant-a.bad type=kv_store \
  capability.replication.factor=2 \
  capability.replication.anti_affinity=region
```

Expected: refused before allocation, `PlacementUnsatisfiable` with required 2 vs available 1 for `region`, no replica created (Q5, SC-003).

## 3. Quorum from any node — SC-005, SC-006, SC-007

Write with no level (default `TWO`) through db-1 (holds a replica) and through a coordinator that is asked the same key after stopping nothing:

```bash
# protocol or CLI mutate; inspect last execution
spacestorage --admin 127.0.0.1:7700 executions --limit 1
spacestorage --admin 127.0.0.1:7710 executions --limit 1
```

Expected: `quorum=TWO`, `source=global`, `achieved=2`, `ack_kind=durable`, `coordinator` named. Identical values from both nodes (SC-007). Read default `ONE`. Explicit `THREE` on RF=3 works; explicit `Acks(4)` rejects `QuorumUnsatisfiable`; a container default of `ALL` on RF=2 clamps when sourced from container (SC-006).

Persistent container with a memory replica: memory `Ack` appears but does not count (Q3).

## 4. Media and memory — SC-004

Bring up `cluster-mixed-media/`. Create with `capability.labels=media=nvme` RF=1 → lands on `nvme-1`. RF=2 NVMe → `PlacementUnsatisfiable` naming the media and excluded `hdd-1`. Memory-mode container → only `nvme-1` (hdd has no pool).

## 5. One replica down — SC-008

Stop db-3. Writes/reads at `ONE`, `TWO`, `QUORUM` succeed; `ALL` fails naming db-3. Restart db-3: replica `behind` then `in_sync`; 0 acknowledged writes lost.

## 6. Partition and heal — SC-009

Isolate internodes to db-3 (`failure_timeout` elapses). Writes on the majority at `QUORUM` succeed; on db-3 at `QUORUM` they fail (cannot gather acks). Heal: LWW (or declared merge) converges; `health` lists resolved conflicts; 0 writes acknowledged on the minority.

## 7. Local vs async remote — SC-010, Q4

`cluster-two-region/` plus `SPACESTORAGE_TEST=1` and `internode { delay { to us-1 200ms; } }` on eu nodes. Container:

```text
group.0 selector=region=eu factor=2 mode=sync
group.1 selector=region=us factor=1 mode=async
each_quorum_policy=refuse
```

`LOCAL_QUORUM` write from eu-1: elapsed does not include 200 ms; US lag visible. `EACH_QUORUM` refused `QuorumRequiresAsyncGroup` until policy `wait`.

Long-RTT variant: `--set cluster.failure_timeout=30m --set replication.hinted_handoff_window=168h` still validates (FR-075, FR-073).

## 8. Shards — SC-011, SC-012

```bash
spacestorage create tenant-a.wide type=kv_store \
  capability.replication.factor=3 \
  capability.replication.anti_affinity=az \
  capability.sharding.key=id \
  capability.sharding.shards=4
```

Every key from every node maps to one shard. Raise shards to 8: still bijection, 0 duplicates. Multi-shard scan with one shard short of quorum fails naming that shard; no partial complete result.

## 9. Rebalance — SC-013, SC-014

Add a fourth node (copy db-1, new name/az/ports). `rebalance` plan lists moves; containers stay available at `QUORUM` throughout; on `done` all `satisfied`. Kill the coordinating node mid-plan: another peer resumes from `cursor`; 0 loss (SC-014). `decommission db-4` add-then-remove; blocked if a pin cannot be honoured.

## 10. 2PC — SC-015

Sharded container with `capability.distributed_transactions`. Atomic batch across two shards; kill one participant after prepare: txn appears `in_doubt`, recovers to a single durable outcome within `txn_timeout`.

## 11. Refusals, stats, capabilities — SC-016–SC-018

Every refusal names container, constraint/level, topology facts, remedy (SC-016). `spacestorage health` and metrics expose lag, repair volume, rebalance volume, ack counts (SC-017). Each of the 13 L1 capabilities is either executed in this walkthrough or refused with a named code — none accepted-and-ignored (SC-018).

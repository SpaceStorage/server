# Quickstart: cluster/namespace Raft, restore, minority refuse

**Feature**: `006-control-plane`

Proves SC-001, SC-002, SC-003, SC-004, SC-005, SC-007, SC-008, SC-011 on the first binary. SC-006 needs an ordered type (complete product). SC-009 / SC-010 need slice 7 (`controlplane-ops`).

Prerequisites and one-node / three-node start: [016 quickstart](../../016-mvp-and-nongoals/quickstart.md). Append [raft-block.conf](contracts/fixtures/raft-block.conf) for loopback election timeouts (or use production defaults and wait longer).

## 0. Config validation

```bash
spacestorage validate specs/006-control-plane/contracts/fixtures/invalid/raft-heartbeat-zero.conf
# expected: exit 2, heartbeat 0 rejected

spacestorage validate specs/006-control-plane/contracts/fixtures/invalid/raft-election-timeout-le-heartbeat.conf
# expected: exit 2, election_timeout must be > heartbeat

spacestorage validate specs/006-control-plane/contracts/fixtures/invalid/exclusive-data-on-first-binary.conf
# expected: exit 2, Slice7Required / unknown_directive on first-binary profile
```

## 1. One-node cluster Raft (SC-001 subset)

Start the one-node starter. Namespace used by PostgreSQL (`acme`) MUST have a namespace Raft group.

```bash
spacestorage controllers --output json
# expected: cluster primary = this node; secondaries [];
#           one namespace group with a primary

psql -h 127.0.0.1 -p 5432 -U demo -d acme -c 'CREATE TABLE t (k int PRIMARY KEY, v text);'
psql -h 127.0.0.1 -p 5432 -U demo -d acme -c "INSERT INTO t VALUES (1, 'x');"
# KV/SQL writes MUST NOT require a leadership lease (SC-005)
```

## 2. Three-node election and failover (SC-001, SC-002, SC-008)

Bring up three members as in `016`. After the third join, `spacestorage controllers` shows **three cluster voters**, one primary, at least one secondary.

Stop the primary process. Remaining voters elect. Membership list unchanged.

```bash
spacestorage controllers --output json | jq '.[] | {group, primary, secondaries}'
# scrape /metrics on a voter: spacestorage_leader_elections_total increased (SC-008)
```

## 3. Minority cannot join or create a namespace (SC-003)

Partition so only one of three cluster voters is reachable (plus any extra non-voters). Through that minority:

```bash
# admit/join a fourth node, or CREATE DATABASE / admin create-namespace
# expected: Minority { group: cluster } (or protocol unavailable), not a silent success
```

Heal. Majority side remains the source of truth.

## 4. Non-voter does not block metadata (SC-001 scenario 5 — complete product / extra members)

If a fourth member exists (learner): stop it. Create a namespace / schema on the majority. MUST succeed. Partition two of three voters away: metadata writes MUST fail even if the learner is up.

## 5. Restore split (SC-004)

On a node with persistent table data, unreplicated memory-mode container, and (optional) replicated memory-mode:

```bash
# restart spacestoraged
psql … -c 'SELECT * FROM t;'          # rows present
# describe memory-mode unreplicated: definition present, content empty, volatility notice
# replicated memory-mode: content may return via 004, not invented locally
```

A process **not** in membership must not vote (`FR-010`).

## 6. Metadata read on a secondary (SC-007)

```bash
spacestorage controllers --endpoint <secondary-admin>
# list namespaces matches primary
```

DDL sent to the secondary is forwarded or `NotLeader`, not applied only there.

## 7. Aggregator identity (SC-011)

With a replicated `K/V Store` in `acme`, scrape `/metrics` on the **namespace primary**: merged shared-datatype series present. On the cluster primary (if different node) those merged series are **not** the aggregator of record. Local series exist on every node.

## 8. Slice 7 (optional)

`spacestorage controllers voters replace --group cluster --from <C> --to <D>` — store unchanged; D votes; C does not (SC-009).

`controller_exclusive_data on` after drain — new tenant replicas skip voters (SC-010).

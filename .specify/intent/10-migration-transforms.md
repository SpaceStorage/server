---
speckit_command: specify
suggested_slug: migration-transforms
source: server/start
read_after: 00-constitution.md
---

# Feature: Data migration and type/model transforms

Specify moving data between nodes and namespaces, and transforming data between datatypes and storage models.

## What

SpaceStorage MUST implement mechanics to **migrate data between nodes and namespaces**, with the opportunity to specify **different migration strategies and policies**.

It MUST also implement **data transforms between datatypes and storage models** (for example L2/L3/L4 types in `03`).

Background jobs for data transformation, data migration, data backup, and data restore are observable (series in `08`); this feature owns the **behavior**: strategies, policies, and transform rules.

Replication (sync/async, quorum) remains placement (`04`). Migration is an explicit, policy-driven move or copy, including cross-namespace, not only replica repair.

## Why

Clusters rebalance, tenants move, and a document store may need to become a relational table or a vector collection without dump/reload outside SpaceStorage.

## Actors

- Cluster administrator migrating a datatype to other nodes or drives
- Tenant migrating data into another namespace under policy
- Operator applying a transform from one datatype/storage model to another
- Background job runner executing migration/transform/backup/restore

## Requirements

- Node-to-node migration with selectable strategies and policies.
- Namespace-to-namespace migration with selectable strategies and policies.
- Transforms between datatypes and between storage models.
- Jobs MUST be specifiable and observable (job names align with `08` background jobs: data transformation, data migration, data backup, data restore).

## Out of scope for this feature

- Ongoing replica streaming (`04`)
- Raft membership changes (`06`) except as a consumer of topology
- UI for tracking progress (`09`) except that progress MUST be trackable (Cerebro-like replication/migration progress)

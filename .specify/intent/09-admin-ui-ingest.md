---
speckit_command: specify
suggested_slug: admin-ui-ingest
source: server/start
read_after: 00-constitution.md
---

# Feature: Cluster admin UIs and log ingest

Specify operator/client graphical interfaces and the log-to-database ingest paths.

## What

### Cerebro-like interface

The server MUST be accompanied with a **Cerebro-like** interface with graphical representation of:

- database state
- nodes
- shards
- data
- replication progress tracking

### Kibana-like interface

A **Kibana-like** interface MUST configure the cluster **globally** and **per-namespace for clients**. It MUST also show the data in tables, different datatype levels, and so on.

### Log ingest

The server MUST be able to **receive logs and parse them into database data** via:

- **Kafka** (like ClickHouse)
- **syslog** protocol

(Outbound logging to Kafka/syslog for operators/tenants is specified in `08`. This feature is **ingest**: logs become stored data.)

## Why

Operators need a live map of nodes/shards/replication. Tenants need a familiar console to configure their namespace and browse types. Ingesting Kafka/syslog logs turns SpaceStorage into the store for operational and application logs without a separate pipeline database.

## Actors

- Cluster operator watching topology and replication in the Cerebro-like UI
- Tenant configuring a namespace and browsing tables/types in the Kibana-like UI
- Kafka producer / syslog sender whose messages become rows/documents/log-stream data

## Requirements

- Graphical cluster state: nodes, shards, data, replication progress.
- Global cluster config UI and per-namespace client config UI.
- Data browsing across datatype levels, including tables.
- Kafka ingest parsed into database data (ClickHouse-like).
- Syslog protocol ingest parsed into database data.

## Out of scope for this feature

- Metric series definitions (`08`)
- Role model except that UIs MUST respect it (`07`)
- Query engine internals (`05`) — UIs consume query/execution, they do not replace it

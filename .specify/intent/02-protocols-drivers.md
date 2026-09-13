---
speckit_command: specify
suggested_slug: protocol-drivers
source: server/start
read_after: 00-constitution.md
---

# Feature: Wire protocols and datatype-aware drivers

Specify protocol compatibility so existing clients can use SpaceStorage through known APIs, while every protocol sees the full type system.

## What

The server MUST implement these protocols and APIs and work over them:

- PostgreSQL
- Cassandra
- Redis
- Elasticsearch
- ClickHouse
- S3
- WebDAV

In the future it MAY implement its own protocol or add extra protocols.

Each protocol MUST be implemented on a **different port** (strongly recommended / treat as MUST unless a documented exception exists).

Drivers (example: PostgreSQL) MUST know about **all datatypes and their features** and MUST use all types via an **abstract interface**. A protocol MUST NOT hide types that exist in the store.

Storage and drivers MUST allow specifying **quorum level** for a query as in Cassandra. Protocols that do not support quorum MUST accept it via options, or use global defaults: write `ack == 2`, read `ack == 1`. All queries MUST be accompanied by **timeout** and **quorum level** as in Cassandra. If a driver does not support timeout/quorum, use global system defaults.

Every node can receive user requests and operate with data (no protocol-only gateway node).

Query execution MUST work **natively for all protocols** (Elasticsearch, PostgreSQL, and so on) — parser/planner/executor are specified in `05`; this feature requires that each protocol can invoke that stack rather than a protocol-private engine.

## Why

Users keep their existing drivers and tools. SpaceStorage still exposes maps, documents, vectors, objects, and compositions through those wires.

## Actors

- Application using a stock PostgreSQL/Cassandra/Redis/ES/ClickHouse/S3/WebDAV client
- Administrator mapping ports and default quorum/timeout
- Protocol adapter translating wire operations to the abstract datatype interface

## Requirements

- Distinct listen port per protocol.
- Abstract datatype interface shared by all drivers.
- Quorum and timeout on every query path, with defaults when the protocol cannot express them.
- Native query path per protocol into the shared execution layer (not a fake subset that only supports that protocol’s native types).

## Out of scope for this feature

- Full L0–L4 inventory (`03`)
- Implementation of planner/MapReduce internals (`05`)
- Admin UIs (`09`)

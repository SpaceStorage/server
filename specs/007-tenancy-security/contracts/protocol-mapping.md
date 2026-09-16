# Contract: Protocol mapping to the registry

**Feature**: `007-tenancy-security` | Crates: protocol handlers | Spec: FR-007 | Seam: `002` FR-009

## PostgreSQL / ClickHouse / Cassandra

Native database/keyspace **name** = `NamespaceName`. Session bind requires the name to exist in the cluster registry. `CREATE DATABASE` / equivalent → `NamespaceCreate` (`CLUSTER_ADMIN` only). `ALTER DATABASE … RENAME` / equivalent → `NamespaceRename` (`CLUSTER_ADMIN` only). Unauthorized → protocol permission error, not a silent other-namespace. After rename, new connections MUST use the new name; already-open sessions stay on namespace id until disconnect.

## Redis / S3 / WebDAV / Elasticsearch

Namespace comes from credential binding (`14`), which MUST point at a registry row. Unbound → authentication refused. `SELECT` (Redis first binary) remains a no-op **inside** the bound namespace (`16`).

## Cross-namespace

A namespace-bound principal opening another name → refuse (SC-004). `admin` MAY switch per `14`.

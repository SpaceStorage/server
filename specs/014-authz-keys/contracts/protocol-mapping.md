# Contract: Protocol mapping (operators)

**Feature**: `014-authz-keys` | Spec: FR-002 | Research R10 | Drivers: `002`

Companion to [authenticator.md](authenticator.md). Starter docs (`002` FR-043) MUST include this table.

| Protocol | How the session gets a namespace | Unbound `admin` |
|----------|----------------------------------|-----------------|
| PostgreSQL | `psql -d <namespace>` (database name) | Allowed if that namespace exists |
| Cassandra | keyspace name | Allowed if it exists |
| ClickHouse | database name | Allowed if it exists |
| Redis | credential binding only; `SELECT` is not a namespace switch (`016` no-op) | **Refused** |
| S3 / WebDAV / Elasticsearch | credential binding only; bucket/index/path is a container inside the bound ns | **Refused** |
| admin CLI/HTTP | request names a namespace when needed | Allowed |

Crossing namespaces = a **new** session with a bound principal or a native-select protocol. Stock Redis clients never see an admin bypass.

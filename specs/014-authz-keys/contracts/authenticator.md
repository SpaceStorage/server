# Contract: Authenticator (replaces `002` users_file)

**Feature**: `014-authz-keys` | Crate: `authz` | Spec: FR-001, FR-002, FR-013 | Research R3, R4, R10, R11

Handlers MUST NOT keep a protocol-private password file. `auth { users_file }` → validate `UsersFileRemoved`.

## Trait (normative)

```text
authenticate(protocol, identity, secret) -> Result<Session, AuthError>
```

`identity` is a login (password/SCRAM) or a client-cert CN (mTLS). `secret` is a password, SCRAM proof, or bearer token hash lookup.

Until the call succeeds, the session MUST NOT see tenant data.

## Protocol mapping

| Handler | Exchange | Namespace |
|---------|----------|-----------|
| PostgreSQL | SCRAM-SHA-256 | Client database name; must be allowed |
| Redis | AUTH password | Binding only; unbound → `UnboundCredential` |
| Cassandra / ClickHouse | native password → same verifier | Keyspace / database name; must be allowed |
| S3 / WebDAV / Elasticsearch | native (SigV4/Digest/basic) mapped to the same principal | Binding only; unbound → `UnboundCredential` |
| `admin` / `admin-http` | password or bearer (`SessionToken`) | Optional native-select / request field; `CLUSTER_ADMIN` MAY pick any existing ns |
| `internode` / `replication` | join secret + builtin `replication` (`011`/`012`) | Not a tenant session |

S3/WebDAV/ES handlers ship in slice 6; the unbound-refuse rule applies as soon as the handler exists.

## Unbound CLUSTER_ADMIN

Builtin `admin` has no namespace binding. Redis/S3/WebDAV/ES AUTH with that login → `UnboundCredential` (same as any unbound principal). No header, connect option, or Redis `SELECT` bypass. Use a bound principal, or PG/CQL/ClickHouse/admin native select.

## Join secret

Presenting `cluster.token_file` bytes to tenant or admin AUTH → `JoinSecretNotAdmin`.

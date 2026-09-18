# Data Model: Authentication, Authorization, Keys, and Audit

**Feature**: `014-authz-keys` | **Date**: 2026-09-18 | **Spec**: [spec.md](spec.md)

Control records apply from the **cluster** Raft log (`06`/`07`). Wrapped data keys sit on container definitions in **namespace** Raft (`03`). Unwrapped material is process memory only. Validation codes are in contracts.

## 1. PrincipalId / LoginName

| Field | Type |
|-------|------|
| id | UUID, assigned at create, immutable |
| login | unique cluster-wide, case-sensitive, `^[A-Za-z][A-Za-z0-9_]*$`, 1–63 chars; **renameable** |

Same charset class as `007` namespace names. Bindings and audit key by **id**.

## 2. PrincipalRecord (cluster log)

| Field | Type |
|-------|------|
| id | PrincipalId |
| login | LoginName |
| scram | salt, iteration count, StoredKey, ServerKey (never plaintext) |
| credential_generation | u64, starts at 1; +1 on password change |
| enabled | bool, default true |
| created_hlc | Hlc |
| bootstrap | bool, true only for the first CLUSTER_ADMIN mint |

**Invariants**: Unique login index. Duplicate → `LoginExists`. Disable does not delete. Delete is a tombstone; id is not reused. Restart of bootstrap MUST NOT insert a second `bootstrap=true` row.

## 3. Verb (closed)

```text
CLUSTER_ADMIN, NAMESPACE_ADMIN, READ, WRITE, CREATE, DROP,
CONFIGURE, REPLICATE, MIGRATE, AUDIT_READ, METRICS_READ
```

`CLUSTER_ADMIN` **implies** every other verb and every namespace. Bitmask in process; stored as a list of names on custom roles (`007` Role.permissions).

## 4. Role / RoleBinding

Owned by `007` (see that data-model). This feature defines:

| Builtin name | permissions | scope | editable |
|--------------|-------------|-------|----------|
| `admin` | `{CLUSTER_ADMIN}` | cluster | no |
| `replication` | `{REPLICATE}` | cluster (nodes) | no |

Non-admin binding: exactly one `namespace_id`. Unbound + Redis/S3/WebDAV/ES → `UnboundCredential`. `replication` MUST NOT appear as a tenant login. First binary: a namespace-bound non-admin principal has an implicit `{READ, WRITE, CREATE, DROP, CONFIGURE}` on that namespace until slice 7 custom roles replace it.

## 5. Session (in-memory per connection)

| Field | Type |
|-------|------|
| principal_id | UUID |
| namespace_id | optional UUID (bound or native-select) |
| credential_generation | u64 copied at AUTH |
| protocol | handler name |

**Later request**: load PrincipalRecord; if `!enabled` or generation mismatch → refuse; then `authorize(verb, resource)` on **current** bindings. In-flight MAY finish. Rename does not change these fields.

## 6. SessionToken (cluster log, optional admin bearer)

| Field | Type |
|-------|------|
| token_hash | SHA-256 of the presented secret |
| principal_id | UUID |
| generation | u64 at issue |
| expires_hlc | Hlc (TTL default 12 h) |

Password change (generation bump) makes existing tokens fail the generation check.

## 7. MasterKey (file, not Raft)

32 bytes at `keys.master_key_file`. Mode `0600`. Backup = copy the file. Never logged.

## 8. KekRecord (cluster log)

| Field | Type |
|-------|------|
| namespace_id | UUID |
| wrapped | AEAD blob under current master |
| kek_epoch | u64; +1 on master rewrap only (data keys unchanged) |

Created with the namespace. Master rotate rewrites `wrapped` for every row.

## 9. DataKey (on container definition)

| Field | Type |
|-------|------|
| key_ref | opaque string id (UUID form recommended) |
| algorithm | `aes-256-gcm` (default) \| `chacha20-poly1305` (`003`) |
| wrapped | AEAD blob under the namespace KEK |
| version | u32; old versions retained until `010` |

`KeyAuthority::resolve` returns the version set. Lost/missing → `KeyUnresolvable{key_ref}`; container unavailable, no plaintext.

## 10. AuditEntry (cluster log)

| Field | Type |
|-------|------|
| id | UUID |
| principal_id | UUID (or nil for failed AUTH with unknown login) |
| login_at_event | optional string |
| action | closed string (see [audit.md](contracts/audit.md)) |
| target | string (node name, key_ref, login, namespace id, …) |
| namespace_id | optional UUID |
| time | Hlc |

Tenants need `AUDIT_READ` (slice 7). First binary: `CLUSTER_ADMIN` reads all.

## 11. State transitions

```text
Principal: created → (rename login)* → (password change / generation++)*
         → disabled → (enabled) | tombstoned

Master:    file present → rotate (rewrap KEKs) → old file invalid
DataKey:   vN current → issue vN+1 (both readable) → 010 drops vN
Session:   unverified → authenticated → later request ok | later request refuse
           (no forced TCP close)
```

## 12. Validation codes (normative names)

| Code | When |
|------|------|
| `BootstrapAdminRequired` | bootstrap without admin_login / password file |
| `JoinSecretNotAdmin` | join secret presented to admin or tenant AUTH |
| `UnboundCredential` | Redis/S3/WebDAV/ES AUTH with no namespace binding |
| `ReplicationNotTenant` | replication identity on a tenant handler |
| `LoginExists` / `LoginNotFound` | unique index |
| `BuiltinRoleImmutable` | edit/delete `admin` or `replication` |
| `RoleNameReserved` | custom named `admin`/`replication` |
| `Slice7Required` | custom role / AUDIT_READ grant on first-binary profile |
| `AuthGenerationMismatch` | later request after password change |
| `PrincipalDisabled` | later request or AUTH while disabled |
| `MasterKeyRequired` / `MasterKeyPermissions` | missing or not 0600 |
| `KeyUnresolvable` | named `key_ref` cannot unwrap |
| `KeyMaterialForbidden` | inline key/cert in config or describe |
| `TransportOmitted` | already `001`; still a startup error |
| `AdminTokenRemoved` | leftover `001` `admin.token_file` as auth |
| `UsersFileRemoved` | leftover `002` `auth.users_file` |

## 13. Relationships

```text
PrincipalRecord 1──0..1 RoleBinding (non-admin: exactly one namespace)
                 └──* Session / SessionToken (generation must match)

NamespaceRecord (`007`) 1──1 KekRecord
Container (`003`) 1──1..* DataKey version (wrapped under that KEK)

CLUSTER_ADMIN ──implies── all Verb
AuditEntry *── principal_id (optional on auth.fail)
```

# Contract: Admin API and CLI

**Feature**: `014-authz-keys` | Crates: `admin-proto`, `spacestorage` | Auth: principal store (this feature)

Parity over `admin` and `admin-http` (`001` FR-020). Exit 4 = authorization (`NotClusterAdmin`, `UnboundCredential`, `AuthGenerationMismatch`, …).

## Ops

| Op | Slice | Notes |
|----|-------|--------|
| `AuthLogin { login, password }` | 1–5 | Returns bearer; optional |
| `PrincipalList` / `PrincipalCreate` / `PrincipalDescribe` | 1–5 | CLUSTER_ADMIN |
| `PrincipalRename` / `PrincipalPasswordSet` / `PrincipalDisable` | 1–5 | see [principals.md](principals.md) |
| `KeysStatus` | 1–5 | master present?; KEK epochs; **no material** |
| `KeysRotateMaster { new_file }` | 1–5 | rewrap KEKs |
| `KeysBind { namespace, container, key_ref, algorithm }` | 1–5 | CLUSTER_ADMIN |
| `KeysRotateData { container }` | 1–5 | new version; rewrite is `010` |
| `AuditList` | 1–5 | CLUSTER_ADMIN; slice 7 also AUDIT_READ |
| `RolePut` custom | 7 | else `Slice7Required` |

## CLI

| Command | Notes |
|---------|--------|
| `spacestorage login` | prints bearer |
| `spacestorage principals` | list |
| `spacestorage principal create <login> --namespace <ns> --password-file P` | |
| `spacestorage principal rename <login> <new>` | |
| `spacestorage principal passwd <login>` | |
| `spacestorage keys` | status |
| `spacestorage keys rotate-master --new-file P` | |
| `spacestorage audit` | table: time, principal, action, target |

`--output json` supported. Restore with specified key remains `013` `spacestorage restore --key/--master-key-file`.

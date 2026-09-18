# Contract: Principals

**Feature**: `014-authz-keys` | Crate: `authz` | Spec: FR-001, FR-014, FR-015, FR-016 | Research R2, R4, R8, R12

## Store

Cluster-log `PrincipalRecord`. Unique login index. Id immutable. Same login charset as `007` namespace names.

## Bootstrap

On `cluster { bootstrap; }` with **no** persisted cluster identity:

1. Require `admin_login` and a readable `admin_password_file`.
2. Derive a SCRAM-SHA-256 verifier; never persist plaintext.
3. Insert one principal (`bootstrap=true`) and bind it to builtin `admin`.
4. Missing login or file → startup `BootstrapAdminRequired`. Admin CLI/HTTP stay closed.

If `identity/cluster.json` already exists, skip mint (no second bootstrap admin). The join secret (`011` `cluster.token_file`) MUST NOT verify as this principal (`JoinSecretNotAdmin`).

## Create

`PrincipalCreate { login, password, namespace_id? }`. `CLUSTER_ADMIN` (first binary). Non-admin principals MUST be bound to exactly one namespace. Unbound principals are only valid when bound to builtin `admin`. `NAMESPACE_ADMIN` create waits for slice 7 and is limited to that namespace.

## Rename

`PrincipalRename { login, new_login }`. Who: the principal themselves; `CLUSTER_ADMIN` (any); `NAMESPACE_ADMIN` for principals bound to their namespace (slice 7). Taken → `LoginExists`. Id, verifier, generation, and `007` bindings unchanged. New AUTH uses the new login. Open sessions keep principal **id** (namespace identity). Rename MUST NOT fail later requests by itself.

## Password

`PrincipalPasswordSet`. Who: self or `CLUSTER_ADMIN` (first binary); `NAMESPACE_ADMIN` in-namespace (slice 7). Increments `credential_generation`. Later requests on sessions that captured the old generation → `AuthGenerationMismatch`. TCP NEED NOT drop. In-flight MAY finish.

## Disable / delete

`PrincipalDisable` / `PrincipalEnable` / `PrincipalDelete`. `CLUSTER_ADMIN` first binary. Disabled AUTH and later requests → `PrincipalDisabled`. Delete tombstones the row; the login becomes free. Id is not reused.

## SCRAM

Stored: salt, iteration count (`auth.scram_iterations`, default 16384), StoredKey, ServerKey. PostgreSQL speaks SCRAM-SHA-256 on the wire (`002`). Redis AUTH presents a password; the server verifies it against the same verifier off-wire.

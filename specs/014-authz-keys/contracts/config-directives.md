# Contract: Config directives

**Feature**: `014-authz-keys` | Crate: `config` | Extends `001` / `011` / `003`

## Bootstrap admin (`011` cluster block)

| Path | Arity | Type | Notes |
|------|-------|------|--------|
| `cluster { admin_login L; }` | 1 on bootstrap | ident | First `CLUSTER_ADMIN` login |
| `cluster { admin_password_file P; }` | 1 on bootstrap | PATH | 0600 password file; verifier stored, not the file contents in Raft |

Required together on first bootstrap. Ignored for minting when cluster identity already exists. Join still uses `token_file` as **join secret**.

## Keys (replaces `003` keyring)

| Path | Arity | Type | Notes |
|------|-------|------|--------|
| `keys { master_key_file P; }` | 1 | PATH | 32-byte master |
| `keys { create_master_if_absent; }` | flag | | Laptop only |

`keys { keyring_file … }` → `KeyringRemoved`.

## Auth

| Path | Arity | Default | Notes |
|------|-------|---------|--------|
| `auth { scram_iterations N; }` | 1 | 16384 | Test may set 4096 |
| `auth { users_file P; }` | — | — | `UsersFileRemoved` |

## Admin token

`admin { token_file }` as **CLUSTER_ADMIN credential** → `AdminTokenRemoved`. (`011` `cluster.token_file` remains join secret.)

## TLS

See [tls.md](tls.md). `client_ca` / `mtls_replace_password` are the only new directives.

## Validation codes

`BootstrapAdminRequired`, `MasterKeyRequired`, `MasterKeyPermissions`, `KeyMaterialForbidden`, `KeyringRemoved`, `UsersFileRemoved`, `AdminTokenRemoved`, `Slice7Required` (custom role directives if any appear in config).

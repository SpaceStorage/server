# Quickstart: Principals, TLS, envelope keys, audit

**Feature**: `014-authz-keys` | **Gates**: SC-001–SC-009 | First binary except where noted slice 7 / `010`

Prerequisites: `001` runtime, `007` starter namespace, `011` bootstrap, `002` PG+Redis. Model: [data-model.md](data-model.md). Commands: [admin-cli.md](contracts/admin-cli.md).

Config: [bootstrap-admin.conf](contracts/fixtures/bootstrap-admin.conf).

## 0. Config validation

```bash
spacestorage validate specs/014-authz-keys/contracts/fixtures/invalid/bootstrap-no-admin.conf
# expected: exit 2, BootstrapAdminRequired

spacestorage validate specs/014-authz-keys/contracts/fixtures/invalid/users-file.conf
# expected: exit 2, UsersFileRemoved

spacestorage validate specs/014-authz-keys/contracts/fixtures/invalid/keyring-file.conf
# expected: exit 2, KeyringRemoved

spacestorage validate specs/014-authz-keys/contracts/fixtures/invalid/admin-token.conf
# expected: exit 2, AdminTokenRemoved

spacestorage validate specs/014-authz-keys/contracts/fixtures/invalid/master-key-inline.conf
# expected: exit 2, KeyMaterialForbidden
```

Omitted `tls`/`plaintext;` on an entrypoint is already `001` (SC-003).

## 1. Bootstrap admin (SC-008)

Start the one-node starter with [bootstrap-admin.conf](contracts/fixtures/bootstrap-admin.conf). Expect: node `ready`; login `admin` works on `spacestorage login`; join secret presented as a password is refused (`JoinSecretNotAdmin`). Restart with `bootstrap;` still declared: still one admin principal.

## 2. Bound tenant on PG and Redis (SC-001)

```bash
spacestorage principal create alice --namespace acme --password-file /tmp/alice.pw
```

`psql -h 127.0.0.1 -p 5432 -U alice -d acme` and `redis-cli AUTH alice <pw>` see only `acme`. `redis-cli AUTH admin <admin-pw>` is refused (`UnboundCredential`). `psql -U admin -d acme` succeeds. `psql -U alice -d otherns` is refused.

## 3. Rename and password re-check (SC-007, SC-009)

Rename `alice` → `ally`. New AUTH uses `ally`; `alice` fails. An open `alice` session still runs as the same id. Change password; the next statement on that session is refused (`AuthGenerationMismatch`); TCP may stay up. In-flight MAY finish.

## 4. Builtins and implicit grant (SC-002)

`CLUSTER_ADMIN` creates a table and writes with no extra grants. `spacestorage` attempt to edit role `admin` → `BuiltinRoleImmutable`. Custom `RolePut` on the first-binary profile → `Slice7Required`. Bound `alice` can SET/GET on Redis (implicit tenant grant). Slice 7: custom READ-only cannot write; custom with `CLUSTER_ADMIN` can.

## 5. Keys (SC-004, SC-005)

Create an encrypted container (algorithm + `key_ref`). Volume snapshot without the master file is ciphertext. `spacestorage keys rotate-master --new-file P` rewraps KEKs; 0 table rewrites. Restore with the specified key unlocks (`013`). Lost master without backup: encrypted containers unrestorable; unencrypted remain.

## 6. Audit (SC-006)

Failed Redis AUTH, a join (`011`), and a key rotate appear in `spacestorage audit` with principal, action, target, time. Slice 7: tenant without `AUDIT_READ` is refused.

## Out of this quickstart

LDAP/SSO, external KMS, data-key rewrite (`010`), UI chrome (`09`), S3/WebDAV/ES unbound refuse (slice 6 handlers, same rule).

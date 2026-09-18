# Contract: Join secret

**Feature**: `011-identity-membership` | Crates: `membership`, `internode` | Spec: FR-001, FR-007 | File: `cluster.token_file` (`004`)

## Generate

Bootstrap creates a cryptographically random secret (≥ 32 bytes), writes it to `token_file` with mode 0600 (Unix), and records epoch 1 as accepted. The operator may copy this file to joining nodes.

## Verify

Before any internodes cluster message: constant-time compare of the presented secret against **all accepted epochs**. Failure → close; do not create pending; do not vote.

Possessing the secret does **not** make the process a member (FR-007).

## Rotate

```text
spacestorage cluster secret-rotate begin
# members accept old + new
spacestorage cluster secret-rotate complete
# old epoch accepted=false
```

If `complete` is not called, `cluster.secret_max_overlap` (default 24h) drops the old epoch.

## Errors

| Code | When |
|------|------|
| `secret_mismatch` | no accepted epoch matched |
| `secret_rotate_in_progress` | second `begin` before `complete` |
| `secret_file_unreadable` | startup / reload |

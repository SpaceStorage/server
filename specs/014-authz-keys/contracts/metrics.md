# Contract: Authz metrics

**Feature**: `014-authz-keys` | Crate: `authz` | Exposition owned by `008`

This crate **increments** these names. It MUST NOT rename or drop series required by `08`.

| Name | Type | Labels | Notes |
|------|------|--------|-------|
| `spacestorage_auth_attempts_total` | counter | `protocol`, `result` | `ok`, `fail`, `unbound` |
| `spacestorage_authz_denied_total` | counter | `verb` | closed verb names |
| `spacestorage_audit_entries_total` | counter | `action` | closed action names |
| `spacestorage_key_unwrap_total` | counter | `result` | `ok`, `missing`, `denied` |
| `spacestorage_key_master_epoch` | gauge | | increments on master rotate |

`result` / `verb` / `action` values are closed. Series also carry `node` as in `08`. No key material, login, or token in labels.

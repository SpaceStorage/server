# Config fixtures for `014-authz-keys`

Valid snippets are appended to `016` / `011` one-node starters. Invalid files are `spacestorage validate` cases (exit 2).

| File | Expected |
|------|----------|
| `bootstrap-admin.conf` | first-node bootstrap + admin + master key |
| `master-key.conf` | `keys { master_key_file }` snippet |
| `invalid/bootstrap-no-admin.conf` | `BootstrapAdminRequired` |
| `invalid/users-file.conf` | `UsersFileRemoved` |
| `invalid/keyring-file.conf` | `KeyringRemoved` |
| `invalid/admin-token.conf` | `AdminTokenRemoved` |
| `invalid/master-key-inline.conf` | `KeyMaterialForbidden` |

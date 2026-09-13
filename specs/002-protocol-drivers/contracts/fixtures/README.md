# Fixtures for `002-protocol-drivers`

| File | Purpose |
|------|---------|
| `node-all-protocols.conf` | Starter config with all eight protocol handlers, `query_defaults`, `auth`, `protocols` knobs (SC-010). Contract test: validates with zero errors. |
| `users.example` | Interim users file format (R11). Contract test: parses to 3 principals; `ops` has no namespace and role `admin`. |
| `invalid/two-handlers-one-port.conf` | `postgresql` and `redis` on the same `address:port` → `entrypoint_duplicate_address` (FR-003). |
| `invalid/bad-quorum.conf` | `query_defaults { write_quorum FIVE; }` → `query_defaults_bad_quorum`. |
| `invalid/zero-timeout.conf` | `query_defaults { timeout 0s; }` → `query_defaults_timeout_out_of_range`. |
| `invalid/missing-users-file.conf` | protocol handler declared without `auth { users_file }` → `auth_users_file_required`. |
| `invalid/users-no-namespace` | users file line with `-` namespace and no `admin` role → `users_file_no_namespace`. |

Each `invalid/*` file carries an `# expect: <code>` header consumed by the contract test, as in feature `001`.

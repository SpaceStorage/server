# Contract: Audit log

**Feature**: `014-authz-keys` | Crate: `authz` | Spec: FR-012 | Research R7 | Export: `008` optional channel

## Source of truth

Cluster-log `AuditEntry`. Not `08`. `08` MAY copy entries to the audit log channel when that channel is on (slice 9, off by default). First binary reads from the cluster log.

## Required actions (first binary)

| action | target |
|--------|--------|
| `auth.ok` | login, protocol |
| `auth.fail` | attempted login, protocol, reason |
| `membership.join` / `leave` / `replace` / `token` | node name / token id (`011`) |
| `key.bind` / `key.rotate_master` / `key.rotate_data` | key_ref / namespace |
| `principal.rename` | old → new login |
| `namespace.rename` | executed in `007`; this feature appends |

Later, when those features exist: role changes, backup/restore, migrate/transform, admin config.

Fields: principal id (nil if unknown login), optional `login_at_event`, action, target, HLC (`012`), optional namespace id. Key material MUST NEVER appear.

## Read

`AuditList { since?, until?, action? }`. First binary: `CLUSTER_ADMIN` only. Slice 7: `AUDIT_READ` custom grant; tenant without it → refuse. Tenant sinks MUST NOT receive other namespaces (`008`).

## Tests

SC-006: join, master/data-key rotate, failed AUTH all present with principal, action, target, time.

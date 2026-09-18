# Contract: Membership metrics

**Feature**: `011-identity-membership` | Crate: `membership` | Exposition owned by `008`

This crate **increments** these names. It MUST NOT rename or drop series required by `08`.

| Name | Type | Labels | Notes |
|------|------|--------|-------|
| `spacestorage_membership_members` | gauge | `cluster_uuid` | len(members) |
| `spacestorage_membership_pending` | gauge | `cluster_uuid` | len(pending) |
| `spacestorage_membership_join_total` | counter | `cluster_uuid`, `result` | `admitted`, `pending`, `refused`, `token` |
| `spacestorage_membership_replace_total` | counter | `cluster_uuid`, `result` | `ok`, `live_replace` |
| `spacestorage_membership_decommission_total` | counter | `cluster_uuid`, `result` | `ok`, `blocked` |
| `spacestorage_membership_secret_epoch` | gauge | `cluster_uuid` | current primary epoch |

`result` values are closed. Node-local series also carry `node` as in `08`.

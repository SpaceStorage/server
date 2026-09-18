# Contract: Restore

**Feature**: `013-durability-and-recovery` | Crate: `crates/backup` | Spec: FR-014, FR-015 | Orchestration: `010`

## Destinations

| Mode | Behavior |
|------|----------|
| Same cluster (replace) | Fill the original names after they have **no content** |
| New cluster (DR) | Same fill rules in the destination cluster |

## Overwrite

If a destination container **still has content** and `confirm_drop` is false → `RestoreDestinationHasContent`; dest intact.

`confirm_drop=true` → drop dest (explicit), then fill. Missing name or empty content → fill.

`010` MUST NOT invent a second overwrite path.

## Keys

Restore accepts a specified key ref (`014`) to unlock or re-encrypt. Missing key → fail naming the reference. Lost master/data key without backup ⇒ those encrypted containers unrestorable (stated in the error). Unencrypted containers unaffected.

## Authz

Cross-namespace restore: admin / dual grant (`014`). Snapshot/restore actions are audited (`014`).

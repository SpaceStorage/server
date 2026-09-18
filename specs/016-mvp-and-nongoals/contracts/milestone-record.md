# Contract: Milestone record

**Feature**: `016-mvp-and-nongoals` | Spec: FR-003, SC-004 | Parser: `release-profile::ledger`

## Files

For each shipped milestone:

```text
docs/milestones/<nnn>-<slug>.md     # human changelog
docs/milestones/<nnn>-<slug>.yaml   # machine record
```

Template: `docs/milestones/000-template.md` (created at implement).

## YAML schema

```yaml
slug: first-binary
profile: first-binary          # first-binary | complete-product
implemented: [1, 2, 3, 4, 5]
deferred:
  - { id: 6, still_owed: true, reason: "remaining 015 handlers" }
  - { id: 7, still_owed: true, reason: "Raft, quotas, full authz" }
  - { id: 8, still_owed: true, reason: "query beyond CRUD" }
  - { id: 9, still_owed: true, reason: "full 08 catalog" }
  - { id: 10, still_owed: true, reason: "migration and PITR" }
  - { id: 11, still_owed: true, reason: "UIs and ingest" }
changelog_ref: docs/milestones/001-first-binary.md
```

## Rules

- `implemented` is a prefix `1..=k` (`slice_gap` otherwise).
- `still_owed` MUST be `true` for slices 6–11. Setting `false` is `deferred_marked_cancelled` (intent deletion by another name).
- `profile: first-binary` REQUIRES `implemented == [1,2,3,4,5]` and the deferred set above.
- `profile: complete-product` REQUIRES `implemented == [1..11]` and `deferred: []`.
- The markdown file MUST contain a heading `## Deferred` listing the same ids (human SC-004).

## What this is not

- Not cluster metadata.
- Not a marketing version. Tags MAY be `slices-1-5` not `v1.0.0` if that string would imply seven protocols.

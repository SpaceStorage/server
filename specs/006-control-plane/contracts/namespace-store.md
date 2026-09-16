# Contract: Namespace store

**Feature**: `006-control-plane` | Spec: FR-004, FR-005, FR-007

## Lifecycle

`create_namespace(name)` is a **cluster** commit (adds to the list) then starts `GroupId::Namespace(id)` with voter set = current **cluster** voter set and all other members as learners.

`drop_namespace` is refused while containers exist unless cascade (`07`). Then the Raft dir is closed and removed after the cluster list commit.

## Namespace log MUST contain

- Schemas
- Container definitions and options
- Shared-datatype metadata
- Leadership leases

MUST NOT serialize leaderless writes (`FR-005`).

## Commit rule

Schema, definition, shared-meta, and lease grant/steal require a **majority of that namespace's voters**. Loss of one namespace majority MUST NOT block another namespace (`Story 2` scenario 5).

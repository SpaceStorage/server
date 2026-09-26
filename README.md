# SpaceStorage server

Rust / Tokio monolith for SpaceStorage: a multiparadigm database whose paradigms are **datatypes**, not bolted-on engines. Spec Kit project root is this directory (`server/`), not the parent monorepo.

Parent layout: `spacestorage/` holds `server/` (this tree), `test/`, and `start` (original product-vision dump). Numbered intent under `.specify/intent/` wins for its domain after it exists.

## Status (honest)

A **first-binary skeleton** exists: workspace crates, release-profile / milestone ledger, starter configs, and some handler/runtime wiring. Converge audits still find stubs and unfinished DoD paths. **Do not treat slices 1–5 as done** until `spacestorage-conformance` with `--features first-binary` is green and the milestone ledger matches reality.

First binary ≠ complete product. The complete product still owes the seven-protocol matrix and slices 6–11; those stay deferred, not deleted.

## Spec Kit

| Path | Role |
|------|------|
| `.specify/memory/constitution.md` | Non-negotiable principles (Rust-only, async Tokio, monolith, …) |
| `.specify/intent/01`–`16` | Per-feature intent input |
| `specs/001`–`016` | Specs, plans, contracts, `tasks.md` |
| `.cursor/skills/speckit-*` | Speckit commands for Cursor agents |
| `docs/milestones/` | Shipped / deferred slice ledger |
| `docs/agent-context-graph.md` | Slice ↔ spec ↔ crate map for agents |

Agent instructions: [`AGENTS.md`](AGENTS.md) (Cursor-native). Stub: [`AGENT.md`](AGENT.md).

## First binary vs complete product

- **First binary** (slices 1–5): runtime; types + WAL + master-key; PostgreSQL subset; Redis K/V MUST; membership + internode + three-node quorum. Profile: `first-binary` / tag `slices-1-5`. Not a “v1” of the seven-protocol matrix.
- **Complete product**: constitution + specs `001`–`015` (Cassandra/ES/CH/S3/WebDAV, Raft control plane, full authz, query beyond CRUD, full observability, migration/PITR, admin UIs/ingest).

See `docs/milestones/001-first-binary.md` and `specs/016-mvp-and-nongoals/`.

## Build and test

From this directory:

```bash
cargo build -p spacestoraged
cargo test -p spacestorage-conformance --features first-binary
```

Milestone ledger validation:

```bash
./scripts/check-milestone.sh
# or: cargo test -p spacestorage-release-profile --test ledger
```

Default workspace features are `first-binary` (PostgreSQL + Redis handlers). A `complete-product` job is not a merge gate for slices 1–5.

Starter configs live under `docs/examples/`. Quickstart: `specs/016-mvp-and-nongoals/quickstart.md`.

## Constitution

Read [`.specify/memory/constitution.md`](.specify/memory/constitution.md) before changing architecture or I/O. Blocking durability (fsync) stays off the async worker pool; write quorum acks wait for the durability contract, not an in-memory write alone.

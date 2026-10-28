# SpaceStorage server

Rust / Tokio monolith for SpaceStorage: a multiparadigm database whose paradigms are **datatypes**, not bolted-on engines. Spec Kit project root is this directory (`server/`), not the parent monorepo.

Parent layout: `spacestorage/` holds `server/` (this tree), `test/`, and `start` (original product-vision dump). Numbered intent under `.specify/intent/` wins for its domain after it exists.

## Status (honest)

**Tip milestone** [`docs/milestones/009-full-product`](docs/milestones/009-full-product.md). Slices **1–11** are closed on [`007-complete-product`](docs/milestones/007-complete-product.md) (`still_owed` cleared). [`008-later-not-first`](docs/milestones/008-later-not-first.md) and `009` add live Raft TCP and the later-not-first mounts (L0, legal-hold, named CDC, external KMS, N/N+1, planetary composition, billing estimate). Merge gate:

```bash
cargo test -p spacestorage-conformance --features complete-product
```

Earlier profiles remain useful regressions:

```bash
cargo test -p spacestorage-conformance --features first-binary
cargo test -p spacestorage-conformance --features handlers-complete
```

Remaining work is the `009` Deferred list, not unpaid slices: invoicing and billing export, multi-process `spacestoraged` orchestration beyond the in-process and live-TCP harnesses, further `/ui/*` polish past `014` session login, WAL kill→reboot client-content cases past the existing Redis gate, and auto-bootstrapping `ControlPlane` on every membership start. Openraft `Raft::new` / 0.10, the PostgreSQL Document Store blob gate, and `/ui/*` session login are already closed on the `007` ledger.

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

- **First binary** (slices 1–5): runtime; types + WAL + master-key; PostgreSQL subset; Redis K/V MUST; membership + internode + three-node quorum. Profile: `first-binary` / tag `slices-1-5`.
- **Handlers-complete** (slice 6): Cassandra / ES / ClickHouse / S3 / WebDAV at `015` MUST. Profile: `handlers-complete`.
- **Complete product** (slices 1–11): full constitution + specs `001`–`015` ladder through admin UIs / ingest, plus the `008`/`009` mounts. Profile: `complete-product`. Slice ledger: `007-complete-product`. Product tip: `009-full-product`.

See `docs/milestones/` and `specs/016-mvp-and-nongoals/`.

## Build and test

From this directory:

```bash
cargo build -p spacestoraged
cargo test -p spacestorage-conformance --features first-binary
cargo test -p spacestorage-conformance --features handlers-complete
cargo test -p spacestorage-conformance --features complete-product
```

Milestone ledger validation:

```bash
./scripts/check-milestone.sh
# or: cargo test -p spacestorage-release-profile --test ledger
```

Starter configs live under `docs/examples/`. Quickstart: `specs/016-mvp-and-nongoals/quickstart.md`.

## Constitution

Read [`.specify/memory/constitution.md`](.specify/memory/constitution.md) before changing architecture or I/O. Blocking durability (fsync) stays off the async worker pool; write quorum acks wait for the durability contract, not an in-memory write alone.

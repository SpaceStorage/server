# SpaceStorage server

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/guide/images/logo-on-dark.svg">
  <img alt="SpaceStorage" src="docs/guide/images/logo.svg" width="72" height="72">
</picture>

Rust / Tokio monolith for SpaceStorage: a multiparadigm database whose paradigms are **datatypes**, not bolted-on engines. Spec Kit project root is this directory (`server/`), not the parent monorepo.

Parent layout: `spacestorage/` holds `server/` (this tree), `test/`, and `start` (original product-vision dump). Numbered intent under `.specify/intent/` wins for its domain after it exists.

## Status (honest)

**Tip** [`docs/milestones/009-full-product`](docs/milestones/009-full-product.md): live Raft over internodes TCP, plus production mounts for L0, legal-hold, named CDC, external KMS, the N/N+1 window, planetary composition, and billing estimate. Slices **1–11** are closed on [`007-complete-product`](docs/milestones/007-complete-product.md). [`008-later-not-first`](docs/milestones/008-later-not-first.md) is the record those mounts sit on.

Merge gate:

```bash
cargo test -p spacestorage-conformance --features complete-product
```

Regression gates:

```bash
cargo test -p spacestorage-conformance --features first-binary
cargo test -p spacestorage-conformance --features handlers-complete
```

What remains is the Deferred list in the tip milestone (invoicing export, multi-process orchestration, further `/ui/*` polish, WAL cases past the Redis gate, per-node `ControlPlane` auto-bootstrap). Those are not unpaid slices.

## Spec Kit

| Path | Role |
|------|------|
| `.specify/memory/constitution.md` | Non-negotiable principles (Rust-only, async Tokio, monolith, …) |
| `.specify/intent/01`–`16` | Per-feature intent input |
| `specs/001`–`016` | Specs, plans, contracts, `tasks.md` |
| `.cursor/skills/speckit-*` | Speckit commands for Cursor agents |
| `docs/milestones/` | Slice ledgers through `007`; follow-on `008-later-not-first`; product tip `009-full-product` |
| `docs/agent-context-graph.md` | Slice ↔ spec ↔ crate map for agents |

Product guide: [`docs/guide/index.md`](docs/guide/index.md) (product, architecture pictures, how to run, configuration parameters).

Agent instructions: [`AGENTS.md`](AGENTS.md) (Cursor-native). Stub: [`AGENT.md`](AGENT.md).

## Profiles

Build **`complete-product`**. That feature is the product tip [`009-full-product`](docs/milestones/009-full-product.md) and the merge gate: slices 1–11, live Raft over internodes TCP, and the later mounts (L0, legal-hold, named CDC, external KMS, the N/N+1 window, planetary composition, billing estimate).

`first-binary` and `handlers-complete` are older Cargo features kept so those slice gates still compile. They are not a second product. A plain `cargo build` still selects `first-binary`, because that remains the Cargo default.

## Build and test

From this directory. Workspace `rust-version` is **1.88**.

```bash
cargo build -p spacestoraged -p spacestorage --features complete-product
cargo test -p spacestorage-conformance --features complete-product
```

Regression gates, same suite, older features:

```bash
cargo test -p spacestorage-conformance --features first-binary
cargo test -p spacestorage-conformance --features handlers-complete
```

Milestone ledger validation:

```bash
./scripts/check-milestone.sh
# or: cargo test -p spacestorage-release-profile --test ledger
```

Slices 1–5 walkthrough: [`specs/016-mvp-and-nongoals/quickstart.md`](specs/016-mvp-and-nongoals/quickstart.md). Starters: `docs/examples/`. Full v1 proof (3-node, Redis, live Raft TCP, L0):

```bash
cargo test -p spacestorage-conformance --features complete-product --test quickstart quickstart_full_v1_three_node_raft_and_l0
```

## Constitution

Read [`.specify/memory/constitution.md`](.specify/memory/constitution.md) before changing architecture or I/O. Blocking durability (fsync) stays off the async worker pool; write quorum acks wait for the durability contract, not an in-memory write alone.

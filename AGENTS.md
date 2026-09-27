# Agent instructions — SpaceStorage server

Cursor-native agent context for this Spec Kit project. Short pointer: [`AGENT.md`](AGENT.md). Durable map: [`docs/agent-context-graph.md`](docs/agent-context-graph.md).

## Root and scope

- Always treat **`server/`** as the Spec Kit / Cargo workspace root (`.specify/`, `specs/`, `Cargo.toml`).
- Parent `spacestorage/` also has `test/` and `start` (vision dump). Do not run Speckit or `cargo` against the parent as if it were this project.
- Do **not** claim a milestone complete until conformance is green for the active profile.

## Constitution (non-negotiable)

[`.specify/memory/constitution.md`](.specify/memory/constitution.md) supersedes conflicting local practice.

Especially:

- Rust-only stack; Tokio async/await throughout.
- Database request work MUST NOT block worker threads. Fsync and equivalent MUST run on a dedicated blocking pool; durable write quorum waits for the durability contract (intent `13`), not an in-memory write alone.
- Single-process multithreaded monolith; type-driven multiparadigm; protocol-per-port for the complete product.

`/speckit.plan` MUST include a constitution check. Violations need Complexity Tracking or the change does not proceed.

## Speckit commands

Skills live under `.cursor/skills/speckit-*` (e.g. `speckit-specify`, `speckit-plan`, `speckit-tasks`, `speckit-implement`, `speckit-converge`, `speckit-constitution`).

- **Converge** appends remaining work to `tasks.md` (append-only for Convergence sections). Do not rewrite history of prior converge blocks.
- **Implement** marks tasks `[X]` as it completes them. Do not invent parallel task lists outside `tasks.md`.

One feature at a time. Set the active feature via `.specify/feature.json` (`feature_directory`) or `SPECIFY_FEATURE_DIRECTORY`. Avoid concurrent agents racing the same `feature.json`.

## First-binary implement order

Specify order stays numeric `001`–`016`. **Implement** for the first binary:

1. `001` (runtime / admin)
2. `003` + `013` + `014` master-key subset (types + durability)
3. `002` + `015` first-binary dialect (PostgreSQL then Redis / K/V MUST)
4. `011` → `012` → `004` (membership → internode/time → placement/quorum)

`016` is the release-profile / conformance / ledger seam — it does not replace behavior owned by `001`–`015`.

## Defer (residuals, not unpaid slices)

Tip milestone [`docs/milestones/007-complete-product`](docs/milestones/007-complete-product.md) closed slices **1–11**. Do **not** reopen slice ledgers for residual hardening. Honest leftovers (track in milestone notes, not as `still_owed` slices):

- WAL kill→reboot→re-read of client content; PostgreSQL Document Store blob gate (alongside Redis G11)
- UI `/ui/*` real `014` session auth (vs interim bearer→cluster-admin)
- Multi-node openraft peer RPC over internodes (single-voter `Raft::new` path landed; remote network still stubbed)

Out of scope for residual passes unless explicitly requested: inventing new slice DoD debt.

## Status discipline

- Tip ledger: slices 1–11 **closed**; `cargo test -p spacestorage-conformance --features complete-product` is the full-matrix merge gate.
- First-binary / handlers-complete profiles remain regression gates (`--features first-binary` / `handlers-complete`).
- Prefer marking incomplete residuals accurately over marking tasks `[X]` early or claiming unpaid slices.

## Where truth lives

| Layer | Location |
|-------|----------|
| Principles | `.specify/memory/constitution.md` |
| Intent | `.specify/intent/01`–`16` (`start` is the original dump) |
| Specs / tasks | `specs/001`–`016/` |
| Milestone ledger | `docs/milestones/` |
| Context graph | `docs/agent-context-graph.md` (+ `.json`) |

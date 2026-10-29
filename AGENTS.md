# Agent instructions — SpaceStorage server

Cursor-native agent context for this Spec Kit project. Short pointer: [`AGENT.md`](AGENT.md). Durable map: [`docs/agent-context-graph.md`](docs/agent-context-graph.md).

## Root and scope

- Always treat **`server/`** as the Spec Kit / Cargo workspace root (`.specify/`, `specs/`, `Cargo.toml`).
- Parent `spacestorage/` also has `test/` and `start` (vision dump). Do not run Speckit or `cargo` against the parent as if it were this project.
- Do **not** claim a milestone complete until conformance is green for the active profile.
- Leave git history untouched. Commit message and changelog are written only when a workflow finishes — see [Commits and changelog](#commits-and-changelog).

## Constitution (non-negotiable)

[`.specify/memory/constitution.md`](.specify/memory/constitution.md) supersedes conflicting local practice.

Especially:

- Rust-only stack; Tokio async/await throughout.
- Database request work MUST NOT block worker threads. Fsync and equivalent MUST run on a dedicated blocking pool; durable write quorum waits for the durability contract (intent `13`), not an in-memory write alone.
- Single-process multithreaded monolith; type-driven multiparadigm; protocol-per-port for the complete product.

`/speckit.plan` MUST include a constitution check. Violations need Complexity Tracking or the change does not proceed.

## Speckit workflow

Skills: `.cursor/skills/speckit-*`. Bundled cycle: [`.specify/workflows/speckit/workflow.yml`](.specify/workflows/speckit/workflow.yml) (specify → review → plan → review → tasks → implement). This repo inserts clarify before plan, analyze before implement, and the implement / converge loop after implement. Integration is `cursor-agent` ([`.specify/integration.json`](.specify/integration.json)); invoke skills, not a separate CLI. Numbering is sequential ([`.specify/init-options.json`](.specify/init-options.json)).

One feature at a time. Persist the active directory in `.specify/feature.json`:

```json
{ "feature_directory": "specs/NNN-slug" }
```

`SPECIFY_FEATURE_DIRECTORY` overrides that file. Do not let two agents write the same `feature.json`.

The main agent only orders the workflow. It sets `feature.json`, picks the model for that phase, launches one subagent, reads the report, and starts the next phase. It does not run specify, clarify, plan, tasks, analyze, implement, or converge itself. Each of those skills is a separate subagent with the skill file, the feature directory, and the model in its prompt. Review gates stay with the main agent: approve, reject, or stop. In the implement / converge loop, implement and converge are separate subagent runs; the main agent starts the next implement only when that converge report appended tasks and proposed `/speckit-implement`.

**Models.** Every phase runs on a medium model: `deepseek-v4.1-flash` or Cursor Composer. Plan, Analyze, and Converge run on a frontier model: Grok, GLM-5.3, or another frontier model of that class. Pass that model when launching the subagent. Do not run Plan, Analyze, or Converge on a medium model.

### New feature

1. **Specify** (`speckit-specify`, medium). Input is `.specify/intent/NN-*.md` or a description. Writes `specs/<NNN>-<slug>/spec.md` and `feature.json`. Pin `SPECIFY_FEATURE_DIRECTORY` when the directory already exists; an unpinned specify allocates the next number and must not be used to refresh `001`–`016`.
2. **Clarify** (`speckit-clarify`, medium). Required before plan. Scan the spec for underspecified decisions, ask at most five questions, and write the answers back into `spec.md`. If the subagent returns questions, the main agent gets the answers and sends them back into that same clarify subagent before planning. Script: `.specify/scripts/bash/check-prerequisites.sh --json --paths-only`.
3. **Review the spec** (`workflow.yml` gate `review-spec`). Reject aborts. Do not plan an unapproved spec.
4. **Plan** (`speckit-plan`, frontier). Script: `.specify/scripts/bash/setup-plan.sh --json`. Constitution check is mandatory (see above). Phase 0 writes `research.md`. Phase 1 writes `data-model.md`, `contracts/`, `quickstart.md`. Re-check the constitution after design.
5. **Review the plan** (gate `review-plan`). Reject aborts.
6. **Tasks** (`speckit-tasks`, medium). Script: `.specify/scripts/bash/setup-tasks.sh --json`. Writes `tasks.md` only. No parallel task list.
7. **Analyze** (`speckit-analyze`, frontier). Required before implement. Read-only consistency check across `spec.md`, `plan.md`, and `tasks.md`. The main agent applies any fix the report requires, through the owning phase's subagent, before implement. `speckit-checklist` stays a reviewer gate, not a workflow phase.
8. **Implement** (`speckit-implement`, medium). Script: `.specify/scripts/bash/check-prerequisites.sh --json --require-tasks --include-tasks`. If `checklists/` has any `[ ]`, stop and ask before continuing. Execute phases in order; tests before the code they guard. Mark a task `[X]` only when that task is done. Do not edit checklist files.
9. **Implement / converge loop.** After implement, run `speckit-converge` on a frontier model. Converge is append-only: one new `## Phase N: Convergence` in `tasks.md`, no edits to earlier tasks, `spec.md`, or `plan.md`. When nothing is missing, leave `tasks.md` unchanged and stop the loop. When it appends tasks, it proposes another `/speckit-implement` for that phase only. Run that implement on a medium model, then converge again on a frontier model. Repeat until converge reports clean. Conformance for the active profile must be green before a milestone claim (`cargo test -p spacestorage-conformance --features <profile>`).

Hooks, when `.specify/extensions.yml` exists: `before_*` / `after_*` on specify, clarify, plan, tasks, analyze, implement, and converge. Skip a hook whose `condition` is non-empty. Run mandatory hooks (`optional: false`); surface optional ones and wait for a yes.

### Existing feature (`specs/001`–`016`)

Spec, plan, and tasks are already there. Do not re-run specify, clarify, plan, or tasks. Set `feature.json` to that directory, then enter the implement / converge loop: implement open Convergence tasks on a medium model, or converge first on a frontier model when an implement has already landed. Each converge that appends a phase proposes the next implement; keep looping until converge writes nothing. Run clarify only when the spec is reopened, and then re-plan on a frontier model before tasks.

| Step | Skill | Model | Writes |
|------|--------|-------|--------|
| Specify | `speckit-specify` | medium | `spec.md`, `.specify/feature.json` |
| Clarify | `speckit-clarify` | medium | answers in `spec.md` (at most five questions; before plan) |
| Plan | `speckit-plan` | frontier | `plan.md`, `research.md`, `data-model.md`, `contracts/`, `quickstart.md` |
| Tasks | `speckit-tasks` | medium | `tasks.md` |
| Analyze | `speckit-analyze` | frontier | nothing (report) |
| Implement | `speckit-implement` | medium | code; `[X]` on finished tasks |
| Converge | `speckit-converge` | frontier | a new Convergence phase and a proposal to run `speckit-implement` again, or no write when clean |

Also available, not part of the default cycle: `speckit-constitution`, `speckit-taskstoissues`.

Open `[ ]` rows in older phases are often unmarked history. The live remainder is the latest Convergence section plus the milestone ledger. Do not mass-check historical tasks. Do not reopen closed slice ledgers for residual hardening.

## Commits and changelog

Agents do not commit. Do not run `git commit`, `git push`, `git amend`, or any other command that creates or publishes a commit. Leave the working tree for the human to commit.

Subagents do not draft the message or touch the changelog. The main agent does both once, when a workflow finishes: a new-feature cycle that has reached a clean converge, or an existing-feature implement / converge loop that stops. Until then, write neither.

1. **Commit message.** Draft one message (subject and short body) in the final report, in the style of recent commits in this repo. The human applies it. Do not commit it.
2. **Changelog.** Create or append [`CHANGELOG.md`](CHANGELOG.md) at the workspace root. One entry per finished workflow:

```markdown
## YYYY-MM-DD — specs/NNN-slug

- What the workflow changed, in a few lines.
```

Do not commit `CHANGELOG.md` either.

## First-binary implement order

Specify order stays numeric `001`–`016`. **Implement** for the first binary:

1. `001` (runtime / admin)
2. `003` + `013` + `014` master-key subset (types + durability)
3. `002` + `015` first-binary dialect (PostgreSQL then Redis / K/V MUST)
4. `011` → `012` → `004` (membership → internode/time → placement/quorum)

`016` is the release-profile / conformance / ledger seam — it does not replace behavior owned by `001`–`015`.

## Defer (residuals, not unpaid slices)

Slice ledger [`docs/milestones/007-complete-product`](docs/milestones/007-complete-product.md) closed slices **1–11**. Do **not** reopen slice ledgers for residual hardening. Full-v1 tip is [`docs/milestones/009-full-product`](docs/milestones/009-full-product.md). Honest leftovers live in that file's Deferred section, not as `still_owed` slices:

- Invoicing / billing export
- Multi-process `spacestoraged` orchestration beyond the in-process and live-TCP harnesses
- Further `/ui/*` polish past `014` session login
- WAL kill→reboot client-content cases past the existing Redis gate
- Auto-bootstrap of `ControlPlane` on every membership start

Openraft `Raft::new` / 0.10, multi-node peer RPC, the PostgreSQL Document Store blob gate, and `/ui/*` `014` session login are **done**. Later-not-first surfaces are recorded in [`docs/milestones/008-later-not-first`](docs/milestones/008-later-not-first.md).

Out of scope for residual passes unless explicitly requested: inventing new slice DoD debt.

## Status discipline

- Tip ledger: slices 1–11 **closed**; full-v1 tip [`docs/milestones/009-full-product`](docs/milestones/009-full-product.md).
- `cargo test -p spacestorage-conformance` is the merge gate. The daemon and CLI have one build; there is no smaller feature set.
- Prefer marking incomplete residuals accurately over marking tasks `[X]` early or claiming unpaid slices.

## Where truth lives

| Layer | Location |
|-------|----------|
| Principles | `.specify/memory/constitution.md` |
| Intent | `.specify/intent/01`–`16` (`start` is the original dump) |
| Specs / tasks | `specs/001`–`016/` |
| Milestone ledger | `docs/milestones/` (tip: `009-full-product`) |
| Workflow changelog | `CHANGELOG.md` (appended when a workflow finishes; not committed by the agent) |
| Context graph | `docs/agent-context-graph.md` (+ `.json`) |

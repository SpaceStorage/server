//! How to record a SpaceStorage implementation milestone.
//!
//! Tags MAY be `slices-1-5` (not `v1.0.0`) when the artifact is the first
//! shippable binary — never brand slices 1–5 as a “v1” of the seven-protocol matrix.
//!
//! ## Files
//!
//! For each shipped milestone create:
//!
//! - `docs/milestones/<nnn>-<slug>.yaml` — machine record
//! - `docs/milestones/<nnn>-<slug>.md` — human changelog with a `## Deferred` heading
//!
//! See [000-template.md](000-template.md) and
//! `specs/016-mvp-and-nongoals/contracts/milestone-record.md`.
//!
//! ## Skip policy
//!
//! Skipping a later slice in an implementation milestone is a **DeferredSlice**
//! (`still_owed: true`), not deletion of `.specify/intent/01`–`15`. Intent files
//! stay. Slice 6’s gate is the **`015` complete-product MUST column**, not the
//! first-binary dialect document.
//!
//! ## CI
//!
//! Default CI is `--features first-binary` (or default features). A
//! `complete-product` job MUST NOT be a merge gate for slices 1–5.
//!
//! Validate ledger records with:
//!
//! ```bash
//! ./scripts/check-milestone.sh
//! ```
//!
//! which wraps `cargo test -p spacestorage-release-profile --test ledger`.

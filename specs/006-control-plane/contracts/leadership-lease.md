# Contract: Leadership lease (epoch fence)

**Feature**: `006-control-plane` | Spec: FR-006, SC-006 | First binary: not required for KV / Relational / Document

## Grant

`lease_grant(container) -> { holder, epoch }` is a **namespace** commit. `epoch` = previous+1 (or 1). Holder SHOULD be a replica in the source `quorum_domain` (`012`); this crate does not pick placement (`04`).

Leaderless types (`003` descriptor: no single-writer / total-order) → `LeaseForbidden`.

## Use

Ordered append / `004` FR-078 write includes `lease_epoch`. Replica applies iff `lease_epoch == current`. Else `StaleEpoch { have, need }` and the record MUST NOT enter the ordered history. LWW/`HLC` MUST NOT be used to merge two ordered streams.

## Steal

Namespace majority may grant a new epoch to a new holder if the old holder is unresponsive (`012` failure detector). Old holder with stale epoch is refused even if it still believes it is leader. Wall-clock timeout is **not** a sufficient fence.

## First binary

Types in `016` slices 1–5 MUST NOT take a lease. The API may exist; conformance SC-006 is complete-product / ordered type.

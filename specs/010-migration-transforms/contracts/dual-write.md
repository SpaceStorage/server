# Contract: Dual-write, catch-up, cutover

**Feature**: `010-migration-transforms` | Crate: `crates/migrate` | Spec: FR-003 live window, FR-008

## Interceptor

At most one `DualWriteWindow` per source container (`JobInProgress` otherwise).

Client write during the window:

1. Apply on source at the container write quorum (durable filter `013` / `012`).
2. Apply **mapped** write on target at the target's write quorum.
3. Ack the client only if both succeed.
4. If (2) fails: do **not** ack; set `paused=true`; error named on the job.

## Catch-up

Scan source for `seq <= install_seq`. Upsert onto target idempotently (same logical key + seq is a no-op). Progress `last_applied_source_seq`.

Sequence numbers are the container source-log / HLC seq (`012`), not byte offsets.

## Cutover / swap gates

All must hold:

- `last_applied_source_seq` ≥ current source head
- Destination quota re-check (`07`)
- Placement re-check (`04`)
- Target marked complete
- Public dest name free, or is the swap source name

Then unregister interceptor, publish names, drop per policy.

## Offline / snapshot strategies

`offline`: interceptor is a **write refuse** on source until copy done.  
`snapshot`: no interceptor; `13` restore is the copy.

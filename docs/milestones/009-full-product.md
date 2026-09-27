# Milestone: full-product (v1 tip)

**Profile**: `complete-product`  
**Tag**: `full-product-v1-2026-09-27`  
**Parents**: [`007-complete-product`](007-complete-product.md), [`008-later-not-first`](008-later-not-first.md)

## Definition

**Full v1** = tip 007/008 closed + production mounts for later-not-first surfaces + live multi-node control plane over real internodes. Product **non-goals** stay deferred (not unpaid slices).

## Implemented

### Track 1 — Live multi-node Raft over internodes

- Default peer path: `BasicNode.addr` → TCP dial (`internode::raft_client`) + fabric `RaftPeerHandler` dispatch
- Proofs: 3-node live TCP election + `client_write`; partition heal; follower restart catch-up; snapshot wire install
- In-process registry harness retained for `inproc://` tests
- Helper: `mount_raft_handler_on_fabric`

### Track 2 — Later-not-first production mounts

| # | Surface | Mount |
|---|---------|--------|
| 1 | L0 creatable | Node `later` + `/v1/l0/containers` + CLI `spacestorage l0` + `l0_workflow.rs` |
| 2 | Legal-hold/erase | `/v1/legal/*` + `migrate::erase_with_legal` + CLI `legal` + `legal_hold_erase.rs` |
| 3 | Named CDC | `/v1/cdc/streams` + CLI `cdc` + `named_cdc.rs` |
| 4 | External KMS | `keys { external_kms {…} }` + Vault-style HTTP unwrap + `/v1/kms/status` + `external_kms.rs` |
| 5 | N/N+1 window | `compat_version_window.rs` (accept N+1 / refuse N+2 + membership join) |
| 6 | Planetary L4 | `/v1/compositions` + CLI `composition` + `planetary_composition.rs` |
| 7 | Billing formula | `/v1/billing/estimate` + CLI `billing` + `billing_estimate.rs` (no invoicing) |

### Track 3 — Operator path + merge gate

- Quickstart: `quickstart_full_v1_three_node_raft_and_l0` (3-node + Redis + live Raft TCP + L0)
- Merge gate remains: `cargo test -p spacestorage-conformance --features complete-product`

## Deferred (non-goals only)

Honest product non-goals — **not** `still_owed` slices:

- Invoicing / billing export pipelines (intent 16 money formula only)
- Full multi-process `spacestoraged` orchestration beyond in-process / live-TCP harnesses
- UI `/ui/*` polish beyond `014` SessionToken login (residual hardening)
- WAL kill→reboot client-content edge cases beyond existing Redis gate
- Inventing new slice DoD debt
- Per-node `ControlPlane` auto-bootstrap on every membership start (mount helper + TCP proofs shipped; operator wiring may follow)

## Gates

```bash
cargo test -p spacestorage-conformance --features first-binary
cargo test -p spacestorage-conformance --features handlers-complete
cargo test -p spacestorage-conformance --features complete-product
```

## Changelog

- **2026-09-27 Track 1**: live TCP openraft peer RPC default mount + conformance proofs
- **2026-09-27 Track 2**: production mounts for all seven later-not-first surfaces
- **2026-09-27 Track 3**: full-v1 quickstart + this tip milestone

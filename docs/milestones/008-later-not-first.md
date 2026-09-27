# Milestone: later-not-first product surfaces (post-007)

**Profile**: `complete-product` (residuals; tip ledger slices 1–11 stay closed)  
**Tag**: `later-not-first-2026-09-27`  
**Parent tip**: [`007-complete-product`](007-complete-product.md)

## Implemented (2026-09-27)

### A — Multi-node openraft peer RPC (was 007 residual)

- `PeerRaftRegistry` + `PeerNetworkFactory` / `PeerNetwork` (`RaftNetworkV2`): Vote / AppendEntries / full Snapshot
- **Default production path**: dial `BasicNode.addr` over live internodes TCP (`raft_client::raft_rpc` + join-secret auth); `inproc://` falls back to registry for same-process harnesses
- Every peer RPC frames through `spacestorage_internode::raft_wire` (`MSG_RAFT_VOTE` / `APPEND` / `SNAPSHOT`)
- `dispatch_peer_rpc` + `RaftPeerHandler` / `RegistryRaftHandler` for inbound internodes → openraft core (fabric returns response JSON)
- Proofs: in-process `three_node_openraft_election_and_client_write`; live TCP election + `client_write`; partition heal; follower restart catch-up; snapshot wire install

### B — Intent “later, not first” product surfaces

| # | Item | Landed |
|---|------|--------|
| 1 | Full L0 creatable-as-workflow | `crates/types` `l0` — 15 creatable + refuse `wal`/…; `L0Catalog` create/put/get; `TypeCatalog::complete_product` |
| 2 | Planetary federated / union / MV | `crates/types` `composition` — L4 kinds + `PlanetaryPlacement` + federated route / MV refresh |
| 3 | External KMS | `crates/crypto` `kms` — `MasterProvider` + `ExternalKmsProvider` / file provider (014 FR-008 alternate) |
| 4 | N/N+1 rolling upgrade polish | `crates/compat` `upgrade` + `JoinRequest.product_version` refuse `product_version_window` |
| 5 | Billing money formula | `crates/observability` `billing` — metrics→`ChargeEstimate` (no invoicing) |
| 6 | GDPR / legal-hold | `crates/storage` `legal_hold` — hold blocks erase; erase → tombstone seq |
| 7 | CDC beyond Log Stream+WAL | `crates/types` `cdc` — named CDC stream + consumer offsets |

## Honest gaps / follow-ons

- ~~Peer RPC production path still uses in-process registry~~ — **done** (see Track 1 / [`009-full-product`](009-full-product.md)).
- ~~L0/L4/CDC library-only~~ — **done**: admin HTTP/CLI + conformance fixtures (009 Track 2).
- ~~External KMS live Vault unwrap~~ — **done**: HTTP unwrap + local-cache fallback.
- Billing formula is in-repo; **no invoice export** (intent 16 non-goal).
- ~~Legal-hold migrate gate~~ — **done**: `migrate::erase_with_legal`.
- ~~Mixed N/N+1 conformance fixture~~ — **done**: `compat_version_window.rs`.
- Optional follow-on: auto-mount `ControlPlane` on every membership bootstrap (helper `mount_raft_handler_on_fabric` shipped).

## Gates

Keep green: `first-binary`, `handlers-complete`, `complete-product` conformance features.

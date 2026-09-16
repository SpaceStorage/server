# Contract: Encryption declaration

**Feature**: `007-tenancy-security` | Crates: `tenancy`, `catalog` | Spec: FR-010, FR-011 | Fields: `003`

## Fields (on container definition, namespace Raft)

| Field | Values |
|-------|--------|
| algorithm | `AES-256-GCM` (default), `ChaCha20-Poly1305` |
| key_ref | string reference |
| scope | `drives` (default), `drives_and_memory` |

Unwrap and KEK hierarchy are `14`. This feature **attaches** and **redacts**.

## Rules

- Inline key bytes on create/alter/describe/logs → `KeyMaterialForbidden`.
- Memory-mode + `drives` → `EncryptionScopeInvalid`.
- First binary MUST persist and show algorithm, key_ref, scope (SC-007).
- Who may bind `key_ref`: `NAMESPACE_ADMIN` or `CLUSTER_ADMIN` for that namespace (`14` FR-011). That is a container-option write, not a cluster registry write.

# Contract: Product version window

**Feature**: `015-compatibility-and-limits` | Spec: FR-013 | Internodes: `012`; disk: `013`; types: `003`

## ProductVersion

`u16` major. First binary = **1**. Internodes **frame** major equals product major.

## Window

| Situation | Result |
|-----------|--------|
| Peers N and N+1 (complete product) | allowed |
| Peer N+2 | refuse `product_version_window` (`012` join/internode) |
| N node writes disk format N+1 | refuse `format_too_new` |
| N node creates type with `introduced_in > N` | refuse `type_too_new` |
| Catalog diff N → N+1 | reportable via `003` / `spacestorage catalog diff` |

## First binary

Conformance MUST NOT require two product versions in one cluster. All starter fixtures use version 1.

## Upgrade

Operator rolls N → N+1 one node at a time. N+1 reads disk N. Cluster min version gates writing format N+1 (operator/control plane; details in `013` + admin). This feature tests the **refuse** edges, not the orchestrator.

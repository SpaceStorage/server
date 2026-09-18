# Contract: On-disk format version

**Feature**: `013-durability-and-recovery` | Spec: FR-008 | Mixed-version: `015`

| File | Magic | Current major |
|------|-------|----------------|
| WAL segment | `WAL1` | 1 |
| SSTable (`003` `SSB1`) | `SSB1` | 1 |
| Snapshot manifest | `SNP1` | 1 |

Unknown **major** at open → refuse **node start** (`format_unsupported`), naming the path.

A node running product N MUST NOT write a major introduced in N+1. Adjacent mixed N/N+1: both read major 1; only N+1 may begin writing major 2 after the window in `015`.

Old replica MUST refuse a peer snapshot/WAL of a newer major rather than interpret it.

# Contract: Restore on boot

**Feature**: `006-control-plane` | Spec: FR-009, FR-010, SC-004 | Invokes `013`

## Order (before `ready`)

1. If local disk has a cluster UUID and this process is **not** in the applied membership → start internodes only as needed to **join** (`011`); **do not** vote; **do not** take new replica placements (`FR-010`).
2. Open `{data_dir}/raft/<group>/` for groups this node should vote or learn (from last snapshot of cluster group if present).
3. Apply snapshots + logs → catalog / `ClusterStore` state (definitions and options).
4. Call `013` restore for **persistent and hybrid content**.
5. Memory-mode **content** empty. Description states volatility (`013`/`003`).
6. After internodes: learners catch up; `004` may re-populate replicated memory-mode.

Corrupt Raft files: isolate that group dir, attempt snapshot from a peer, else `node_state` degraded; tenant WAL corruption is `013`.

Unknown Raft format major: refuse start (`015`).

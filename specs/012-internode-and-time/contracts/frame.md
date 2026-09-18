# Contract: Internode / replication frame

**Feature**: `012-internode-and-time` | Crates: `internode`, `replication` | Spec: FR-004, FR-019 | Extends [`004` coordinator](../../004-distribution-placement/contracts/coordinator.md)

## Envelope (both ports)

```text
u32le length | u8 version | u16le msg_type | payload
```

- Current version: **1**.
- Compatibility window: this version and the previous (N/N+1). Version 1 also accepts 0 as “same codec, pre-window”. Outside window → close, `version_incompatible`.
- Internodes payload: serde JSON for control and `FanoutWrite`/`FanoutRead`.
- Replication: JSON **header** then raw length-prefixed bytes for batches.

Unknown internodes `msg_type` → `Ack` `{ error: unknown_message }`; do not tear the mesh (`004`).

## Internode types owned or extended here

| Type | Notes |
|------|--------|
| `Heartbeat` | HLC, internodes RTT nonce, optional placement digest |
| `FanoutWrite` / `FanoutRead` / `Ack` | in-domain only (source replicas) |
| `ForwardWrite` | follower coordinator → source |
| `Promote` / `FenceEpoch` | see [promote.md](promote.md) |
| `DomainAnnounce` | domain membership gossip (also in cluster log) |

`011` join types and `006` Raft types remain additive on internodes.

## Replication types

| Type | Notes |
|------|--------|
| `SourceLogBegin/Record/End` | follower apply in position order |
| `StreamBegin/Chunk/End` | bootstrap, rebuild |
| `RepairChunk` | bulk compare payload (coordination stays on internodes) |
| `HintReplay` | hinted bytes (`004` window) |

## Backpressure

Per-connection send/recv buffers (`internode.*`, `replication.*`). Full → slow or fail **that** stream (`stream_backpressured`). MUST NOT block a Tokio worker. `001` buffer metrics apply.

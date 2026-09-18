# Contract: Counted acknowledgements

**Feature**: `013-durability-and-recovery` | Crates: `storage`, `placement` | Spec: FR-001, FR-002, FR-016 | Who counts: `012`

| Container mode | Counted ack means | Crash of that replica |
|----------------|-------------------|------------------------|
| persistent / hybrid | WAL durable on the replica’s drive | value returns after replay |
| memory | memtable/memory pool only | content gone unless another replica re-populates |

Mixed replica sets: execution record lists `durable` vs `memory` (`004` FR-034). Memory acks never satisfy persistent/hybrid `TWO`/`QUORUM`.

Hybrid: ack covers the write that would live in the memory portion (`WriteBuffer` / `HotSet`); restart rebuilds that portion from WAL + SSTables (`003` HybridPolicy), not `RestoredEmpty`.

# Contract: Metrics this feature increments

**Feature**: `015-compatibility-and-limits` | Names live in `008`; this crate MUST increment, MUST NOT rename labels

| Series | Labels | When |
|--------|--------|------|
| `spacestorage_limit_rejected_total` | `limit`, `protocol`, `namespace` | size/connection reject |
| `query_admission_rejected_total` | existing `005`/`008` | concurrent/memory/spill_disk/buffer |
| `spacestorage_compat_must_not_total` | `protocol`, `verb` | MUST NOT verb |
| `spacestorage_limit_usage_ratio` (gauge, optional warning) | `limit` | MAY at ≥ 0.8; not a substitute for hard reject |

Handshake mismatch counters stay `002`. Golden-signal duration/error labels unchanged.

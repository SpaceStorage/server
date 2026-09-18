# Contract: Failure detector

**Feature**: `012-internode-and-time` | Crate: `internode` | Spec: FR-015 | Consumed by `011` replace and `004` quorum

## Knobs (cluster-wide)

| Knob | Default | Reload |
|------|---------|--------|
| `cluster.heartbeat_interval` | `2s` | live |
| `cluster.failure_timeout` | `15s` | live |

Observers MUST use the same values (cluster config, not per-node override, not per destination group). Phi-accrual MUST NOT be used.

## Event

After last successful internodes heartbeat + `failure_timeout`:

1. Peer is **unavailable** for write/read quorum counting (`004` “counted” replicas).
2. Peer is **replace-eligible** (`011` FR-012). No extra ping.

A still-heartbeating `ready` or `draining` member MUST count and MUST refuse replace.

Heartbeat resume → available again (unless decommissioned or fenced incarnation).

## Crash-stop

Not Byzantine. Hostile tenants are `016`; operator-run nodes. A partitioned minority of **controllers** still cannot mutate membership (`006`).

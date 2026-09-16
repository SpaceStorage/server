# Contract: Config directives

**Feature**: `007-tenancy-security` | Crate: `config` | Spec: FR-013 | Research R2, R11, R16

## Starter namespace (first binary)

```nginx
cluster {
    starter_namespace acme;
}
```

Omitted: no implicit tenant; operator must `namespace create`. Duplicate with an already-applied name: no-op if id/name match.

## Quota block (slice 7)

```nginx
namespace acme {
    quota {
        bytes 10G;
        objects 1000000;
        connections 500;
        ops_per_sec 10000;          # optional
        datatype "K/V Store" bytes 1G;
    }
}
```

Without `tenancy-quotas`: any `quota { }` → `Slice7Required` at validate ([invalid/quotas-on-first-binary.conf](fixtures/invalid/quotas-on-first-binary.conf)). Negative / overflow → `QuotaNegative`.

## Private telemetry

`private_metrics on;` / `private_logs on;` → `ObservabilityRequired` until `08`. Omitted or `off` allowed.

Reload: starter_namespace is bootstrap-only (not a live create of a second name). Quota replaces are live in slice 7 via admin; config reload MAY apply `QuotaReplace` as CLUSTER_ADMIN local node (must still commit on cluster majority).

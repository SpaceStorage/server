# Contract: Configuration Directives Added by This Feature

**Feature**: `011-identity-membership` | Extends [`004` cluster block](../../004-distribution-placement/contracts/config-directives.md) and [`001` grammar](../../001-runtime-cli-api/contracts/config-grammar.md) | Crate: `crates/config`

Reserved words claimed: `bootstrap`, `join`, `seeds` (alias of `004` `peers`). `cluster.token_file` stays the **join secret** file (`004` / research R2).

## Directives

| Path | Arity | Type | Default | Reload | Notes |
|------|-------|------|---------|--------|-------|
| `cluster { bootstrap; }` | flag | literal | — | restart | Exclusive with `join`. Requires empty or self-only seeds |
| `cluster { join; }` | flag | literal | — | restart | Exclusive with `bootstrap` |
| `cluster { seeds { name N; address A; port P; } }` | repeatable | | — | restart | First-join discovery. `peers` accepted as alias |
| `cluster { token_file P; }` | 1 | PATH | — | live re-read | Join secret. Bootstrap **creates** if missing |
| `cluster { join_token_file P; }` | 0–1 | PATH | — | restart | Optional unattended join |
| `cluster { secret_max_overlap D; }` | 1 | DURATION | `24h` | live | Safety cap on rotate overlap |
| `cluster { join_token_ttl D; }` | 1 | DURATION | `12h` | live | Range 1h–72h |

`node { name; }` is `001`/`004`. Ladder keys are `node { labels { } }` (`004`); join refuses if any cluster-ladder key is missing.

## Validation codes

| Code | When |
|------|------|
| `bootstrap_and_join` | both flags |
| `bootstrap_or_join_required` | neither flag and no persisted membership |
| `bootstrap_foreign_seeds` | bootstrap with a seed that is not self |
| `join_secret_required` | `join` without readable `token_file` |
| `join_token_ttl_range` | TTL outside 1h–72h |
| `ladder_key_missing` | checked at join, not only at parse |

Persisted membership (`identity/cluster.json` + member row): `join`/`bootstrap` flags still parsed; missing seeds is **not** a startup error (FR-016).

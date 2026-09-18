# Config fixtures for `015-compatibility-and-limits`

Valid snippets append to `016` one-node starters. Invalid files are `spacestorage validate` cases (exit 2).

| File | Expected |
|------|----------|
| `first-binary.conf` | PG+Redis, default limits, product_version 1 |
| `limits-raised.conf` | operator-raised size/connection caps |
| `invalid/limits-zero.conf` | `limits_zero{knob:"max_key"}` |
| `invalid/product-version-zero.conf` | `product_version_zero` |

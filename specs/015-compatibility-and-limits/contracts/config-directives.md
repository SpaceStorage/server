# Contract: `limits { }` and related config

**Feature**: `015-compatibility-and-limits` | Crate: `crates/config`

```text
limits {
    max_key 1KiB;
    max_value 16MiB;
    max_query_text 1MiB;
    max_result 64MiB;
    max_connections_per_entrypoint 10000;
    max_connections_per_principal 1000;
}

cluster {
    product_version 1;    # test/override; default compiled-in
}
```

`query { max_concurrent_*; max_memory; spill; }` remains `005`. Complete-product starter sets `spill on`.

All `limits` fields optional; missing → defaults. Live-reload: new connections and new queries only.

| Error code | When |
|------------|------|
| `limits_zero{knob}` | any limit is 0 |
| `limits_unknown_unit` | unparsable size |
| `product_version_zero` | `product_version 0` |

Effective config reports provenance `configured | built_in`.

## Admin / CLI

| Surface | Op |
|---------|-----|
| `GET /v1/limits` | current limits + usage ratios |
| `spacestorage limits` | same |
| `spacestorage catalog diff --from N --to N+1` | `003` listing; refuse rules tested here |

Token-protected (`014`).

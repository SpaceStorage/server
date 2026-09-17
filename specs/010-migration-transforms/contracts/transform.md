# Contract: Transform

**Feature**: `010-migration-transforms` | Crate: `crates/migrate` | Spec: FR-003, FR-004, FR-013, FR-014

## Rewrite kinds

`type_model` | `incompatible_schema` | `re_encode` | `re_compress` | `re_encrypt` | `sharding_key`

In-place type/schema/codec-rewrite-of-existing/key/sharding-key stay **refused** by `03`/`04` with error text pointing at this mechanism.

## Shape

Always rewrite into a **new** container (`incomplete` until swap/complete). Source stays readable and writable (`live`). Optional `swap` atomically binds the source public name to the new container.

## Swap and retain

- `swap=true` (default for same-namespace): public name → new container.
- `retain_source=false` (default): drop previous container after swap.
- `retain_source=true`: rename previous to `<name>__pre_transform_<job_id>` if free; else refuse swap `NameExists`.

New-container public name (when not swapping onto source name) MUST be free (FR-013).

## L4 compositions

Default: refuse the composition; transform members first (spec assumption).

## Sharding key

Changing a key `04` marks fixed is this rewrite, not rebalance.

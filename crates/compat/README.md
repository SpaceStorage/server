# spacestorage-compat

Compatibility **ceiling** for SpaceStorage (feature `015-compatibility-and-limits`):

- Dialect profiles: `FirstBinary` ⊂ `HandlersComplete` ⊂ `CompleteProduct` (never branded “v1”)
- MUST / MUST NOT verb matrix + wire versions
- Closed isolation set (`READ COMMITTED` / `SNAPSHOT`; `SERIALIZABLE` refused)
- Size / connection / admission limit types and defaults
- Product-version window N/N+1

## Handler contract

Handlers (`002`) call `classify` / `classify_outcome` **before** IR. `MustNot` uses that
protocol’s not-supported form — never a silent empty success.

Stock **clients** on MUST verbs are in scope. Unmodified **applications** that require
MUST NOT verbs are out of scope.

FirstBinary ships PostgreSQL + Redis only; entrypoints naming ES/Cassandra/CH/S3/WebDAV
fail startup as `unknown_handler`.

## Config fixtures

Contract fixtures live under
`specs/015-compatibility-and-limits/contracts/fixtures/`
(`first-binary.conf`, `limits-raised.conf`, `invalid/limits-zero.conf`,
`invalid/product-version-zero.conf`). Invalid fixtures must fail validate with exit 2
(`limits_zero`, `product_version_zero`).

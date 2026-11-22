# Fixtures

JSON bodies the conformance tests may load. They document the success shape; they are not a live cluster.

| File | Contract |
|------|----------|
| [overview-degraded.json](overview-degraded.json) | `GET /v1/ui/overview` with one not-ready member and one lagging replica named above the fold |
| [namespace-page.json](namespace-page.json) | `GET /v1/ui/namespaces` one-page result, exact container count |
| [audit-page.json](audit-page.json) | `audit_list` / `GET /v1/ui/audit` page |

Tests check required keys and the absence of `token`, `index`, and `document` as field names.

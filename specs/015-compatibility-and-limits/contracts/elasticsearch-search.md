# Contract: Elasticsearch search and aggregations

**Feature**: `015-compatibility-and-limits` | Spec: FR-015 | List lives **here**; `005` implements it

No handler in FirstBinary. Slice 6 (`HandlersComplete`) ships the handler.

## MUST (all profiles that include the handler)

- Index / get / delete document
- Create index with mappings (`002` type option)
- cat / list
- Search `POST/GET /{index}/_search` with top-level `query` one of:
  - `query_string`
  - `match`
  - `term`
  - `range`
  - `bool` whose clauses are only the above

Lowering: `LogicalRequest::Scan` + filter.

## MUST aggregations (CompleteProduct / slice 8 only)

Closed list: `terms`, `min`, `max`, `sum`, `avg`, `histogram`, `value_count`.

Lowering: `LogicalRequest::Aggregate`. Before slice 8: not-supported (`agg_not_in_profile`) even if the JSON is in this list.

## MUST NOT

ILM, ingest pipelines, ML, CCR, `script` queries/aggs, pipeline aggregations, `significant_terms`, `composite`, `date_histogram` (not in the closed list), anything not named above.

Handler returns ES-shaped 400 with `type: "illegal_argument_exception"` (or `002` renderer equivalent). Never 200 + empty hits for a MUST NOT verb.

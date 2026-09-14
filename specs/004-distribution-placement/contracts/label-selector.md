# Contract: Label Selector

**Feature**: `004-distribution-placement` | Crate: `crates/placement` (`selector.rs`) | Spec: FR-011–FR-015

## Grammar

```text
sel   := or
or    := and ( 'or' and )*
and   := not ( 'and' not )*
not   := 'not' not | atom
atom  := '(' sel ')'
       | KEY '=' VALUE | KEY '!=' VALUE
       | KEY 'in' '(' VALUE ( ',' VALUE )* ')'
       | KEY 'notin' '(' VALUE ( ',' VALUE )* ')'
       | 'has' KEY
       | 'media' '=' MEDIA
       | 'memory'
KEY   := IDENT
VALUE := IDENT | STRING
MEDIA := 'nvme' | 'ssd' | 'hdd' | IDENT
```

Whitespace insignificant. Keywords `and or not in notin has media memory` are reserved in selector context only. Case-sensitive keys and values.

## Matching

Evaluated against one `PlacementTarget`: a node plus exactly one of (a named drive, the memory pool).

| Atom | True when |
|------|-----------|
| `k=v` | declared or derived label `k` equals `v` |
| `k!=v` | key present and value differs, **or** key absent |
| `k in (...)` | value in the set |
| `has k` | key present (declared or derived) |
| `media=nvme` | the target drive's media is `nvme` (when targeting a drive) or the node has at least one such drive (when filtering nodes) |
| `memory` | the target is the memory pool and it has remaining bytes for the replica footprint estimate |

A node that does not carry an anti-affinity key is **not** a distinct domain; it is excluded with `missing_anti_affinity_key` (FR-022). `k!=v` does not invent a domain.

## Combinators

`and` / `or` / `not` / parentheses as usual. A container may attach several selectors (media + locality); they are AND-ed (FR-015).

## Persistent placement

`capability.persistent_placement.labels=<sel>` is a pin: rebalance, repair and capacity pressure MUST NOT move replicas off nodes matching `sel`. If the pin cannot host the factor, the container is `unplaceable`, not relocated (FR-014).

## Parse errors

`SelectorSyntax{line,col,msg}`. Unknown media IDENT is allowed (expandable). Empty selector matches every target.

## Examples

```text
az=az1 and media=nvme
region in (eu, uk) and not rack=rack0
has planet and memory
region=eu or region=us
```

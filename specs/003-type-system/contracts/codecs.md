# Contract: Encodings and Compression Codecs

**Feature**: `003-type-system` | Crate: `crates/codec` | Spec: FR-024–FR-026, FR-028

Two open registries. Adding an entry makes it selectable for new containers and changes nothing existing (FR-046).

## 1. Data kinds

`bool`, `integer` (all widths), `float` (`f32`/`f64`), `decimal`, `string`, `bytes`, `timestamp`, `date`, `time`, `uuid`, `json`, `vector`, `geo`, `enum`, `key` (the encoded key stream of ordered types).

## 2. Encoding registry

| Encoding | Applies to | Description | Fallback |
|---|---|---|---|
| `plain` | every kind | fixed-width or length-prefixed values; always applicable | — |
| `gorilla` | `float`, `timestamp` | XOR of successive values with leading/trailing-zero bitstream (Gorilla) | `plain` |
| `delta` | `integer`, `timestamp`, `date`, `time`, `key` | delta-of-delta with zigzag varint | `plain` |
| `dictionary` | `string`, `bytes`, `enum`, `uuid`, low-cardinality `integer` | per-block dictionary of distinct values + symbol stream | `plain` per block on overflow |
| `rle` | `bool`, `enum`, `integer`, and any kind after `dictionary` | run-length over the symbol stream | `plain` |

Rules:

- Encodings are chosen **per field or per data kind** within one container (FR-025); `encoding.<field>` beats `encoding.<data_kind>`.
- An encoding applied to a kind it does not cover is refused as a whole with the applicable list: `EncodingNotApplicable{encoding, data_kind, applicable}` (spec Story 3 scenario 5) — no partial application.
- `dictionary` overflow: when a block's distinct count exceeds `types.dictionary_max_cardinality` (default 65 536), that block falls back to `plain`, sets the `dictionary-fallback` flag in its header, and the container description records that the fallback occurred. Writes are never refused for this reason (spec edge case).
- `rle` may be layered after `dictionary` (`encoding.tags=dictionary+rle`); no other composition is accepted in v1.

## 3. Compression registry

| Codec | Crate | Levels | Default level | Notes |
|---|---|---|---|---|
| `none` | — | — | — | default for `memory` mode |
| `snappy` | `snap` | — | — | fixed effort |
| `zstd` | `structured-zstd` | 1–11 | 3 | pure Rust, frames decodable by upstream zstd; cross-checked against `ruzstd` in property tests |
| `lz4` | `lz4_flex` | — | — | already used by `002` for wire compression |

Rules:

- Compression is per container and applies per block; the block header records the codec, so a container may hold blocks of several codecs after a change (FR-028).
- **Compression always precedes encryption** and the order is not configurable (FR-026, [encryption.md](encryption.md)).
- Level out of range ⇒ `UnknownCodec` with the accepted range (validation stage 7).
- Global defaults: `types { default_compression … }` for persistent/hybrid modes, `none` for memory mode; the type descriptor may override with `default_codec`.

## 4. Changing codecs and encodings after creation (FR-028)

New data uses the new setting; existing data stays readable under the old one; the description lists every codec and encoding present, and the operator is told that rewriting existing data needs a transform (`10`). The block header is what makes this work — every block is self-describing, so no global rewrite is implied.

## 5. Test obligations

- Golden vectors per encoding (including `-0.0`, `NaN`, `i64::MIN`, `u64::MAX`, empty strings, a 1-element run, a 10⁶-element run) and per codec.
- Property tests: `decode(encode(x)) == x` for every applicable data kind; `decompress(compress(b)) == b` for every codec and level; ZSTD frames produced by `structured-zstd` decode under `ruzstd` and vice versa.
- Matrix test: every creatable type × every applicable encoding × every codec round-trips losslessly (SC-004).

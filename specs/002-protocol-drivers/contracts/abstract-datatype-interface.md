# Contract: Abstract Datatype Interface

**Feature**: `002-protocol-drivers` (consumer; interim provider) | Crate: `crates/types` (`spacestorage-types`) | Future owner: feature `03-type-system`

This is the **only** surface through which a protocol driver touches data (spec FR-011, FR-012). Drivers never see storage primitives, encodings, or placement.

## Stability

- Trait signatures below are stable for drivers; `03` may add methods with default implementations and add types to the registry, but MUST NOT remove or rename these.
- The interim inventory (six types, in-memory) is replaced by `03` behind the same traits.

## Registry

```rust
pub struct TypeName(pub &'static str);
pub struct TypeRegistry { /* BTreeMap<TypeName, Arc<dyn Datatype>> */ }
impl TypeRegistry {
    pub fn get(&self, name: &str) -> Option<Arc<dyn Datatype>>;
    pub fn iter(&self) -> impl Iterator<Item = &Arc<dyn Datatype>>;   // stable order: by name
    pub fn register(&mut self, t: Arc<dyn Datatype>) -> Result<(), TypeError /* DuplicateType */>;
}
```

Interim registry contents: `kv_collection`, `relational_table`, `document_store`, `object_collection`, `vector_collection`, `ordered_map`.

## `Datatype`

```rust
#[async_trait]
pub trait Datatype: Send + Sync {
    fn name(&self) -> TypeName;
    fn level(&self) -> Level;                                  // L0..L4
    fn create_options(&self) -> &[CreateOption];               // documented options, drives per-protocol type-option validation
    fn validate_create_options(&self, opts: &Options) -> Result<(), TypeError>;
    async fn open(&self, meta: &ContainerMeta) -> Result<Arc<dyn Container>, TypeError>;
    fn operations(&self) -> &[OperationSpec];                  // type-specific ops (e.g. knn, range, incr)
    fn canonical_schema(&self) -> CanonicalSchema;             // shape of "$d" for this type's values
}
```

`OperationSpec { name, args: CanonicalSchema, result: CanonicalSchema, mutates: bool }`. Drivers build their type-specific surfaces (`spacestorage.<op>()` SQL functions, `SS.<TYPE>.<OP>` Redis commands, `/_spacestorage/<op>` ES routes, `?spacestorage-op=<op>` S3, `REPORT` WebDAV) **from this table**, never from a hard-coded list.

## `Container`

```rust
#[async_trait]
pub trait Container: Send + Sync {
    fn meta(&self) -> &ContainerMeta;
    async fn get(&self, key: &Key) -> Result<Option<CanonicalValue>, TypeError>;
    async fn put(&self, key: Key, value: CanonicalValue, cond: PutCond) -> Result<PutOutcome, TypeError>;
    async fn delete(&self, key: &Key) -> Result<bool, TypeError>;
    async fn exists(&self, key: &Key) -> Result<bool, TypeError>;
    fn scan(&self, q: ScanQuery) -> BoxStream<'_, Result<Row, TypeError>>;
    async fn aggregate(&self, q: AggQuery) -> Result<Vec<Row>, TypeError>;
    async fn type_op(&self, op: &str, args: CanonicalValue) -> Result<CanonicalValue, TypeError>;
    fn object(&self) -> Option<&dyn ObjectOps>;               // Some for byte-stream types (object_collection)
    async fn alter(&self, opts: Options) -> Result<(), TypeError>;
    fn stats(&self) -> ContainerStats;
}
```

- `Key`: `Str(String) | Tuple(Vec<CanonicalValue>) | Bytes(Vec<u8>)`; each type documents which it accepts.
- `PutCond`: `Always | IfAbsent | IfPresent | IfMatch(etag)`; `PutOutcome { created: bool, etag: Option<String>, version: u64 }`.
- `ScanQuery { projection: Option<Vec<FieldPath>>, filter: Option<Expr>, sort: Vec<(FieldPath, Order)>, limit: Option<u64>, offset: u64 }`.
- `AggQuery { group_by: Vec<FieldPath>, aggs: Vec<(String, AggFn, FieldPath)>, filter: Option<Expr> }` with `AggFn ∈ Count | Sum | Min | Max | Avg | CountDistinct`.
- `Row = BTreeMap<String, CanonicalValue>`.
- Writes MUST reject values the type cannot hold with `TypeError::TypeMismatch { container_type, field, value }` and MUST store nothing (spec FR-016).

### `ObjectOps`

```rust
#[async_trait]
pub trait ObjectOps: Send + Sync {
    async fn put_stream(&self, key: Key, meta: ObjectMeta, body: BoxStream<'_, io::Result<Bytes>>, cond: PutCond) -> Result<PutOutcome>;
    async fn get_range(&self, key: &Key, range: Option<Range<u64>>) -> Result<Option<(ObjectMeta, BoxStream<'static, io::Result<Bytes>>)>>;
    async fn head(&self, key: &Key) -> Result<Option<ObjectMeta>>;
    async fn copy(&self, from: &Key, to: Key, meta: Option<ObjectMeta>) -> Result<PutOutcome>;
    fn list_prefix(&self, prefix: &str, delimiter: Option<char>, start_after: Option<&str>, limit: u32) -> BoxStream<'_, Result<ListEntry>>;
    async fn multipart_begin(&self, key: Key, meta: ObjectMeta) -> Result<UploadId>;
    async fn multipart_part(&self, id: &UploadId, part: u32, body: BoxStream<'_, io::Result<Bytes>>) -> Result<PartEtag>;
    async fn multipart_complete(&self, id: &UploadId, parts: Vec<(u32, PartEtag)>) -> Result<PutOutcome>;
    async fn multipart_abort(&self, id: &UploadId) -> Result<()>;
}
```

`ObjectMeta { content_type, content_length, etag, last_modified, user_meta: BTreeMap<String,String> }`. `ListEntry ∈ Object(ObjectMeta) | CommonPrefix(String)`.

## Canonical values

See [canonical-representation.md](canonical-representation.md). Every `CanonicalValue` given to `put`/`type_op` is validated against `canonical_schema()` by the container.

## Errors

```text
enum TypeError { UnknownType{name}, DuplicateType, InvalidCreateOption{name, reason}, MissingCreateOption{name}, TypeMismatch{container_type, field, value}, UnknownOperation{op}, InvalidOperationArgs{op, reason}, NotFound, AlreadyExists, PreconditionFailed, Capacity{buffer}, Internal }
```

Drivers map `TypeError` to their protocol's error form via `protocol-core::ErrorRenderer` (see [data-model.md §8](../data-model.md#8-per-protocol-error-form-errorrenderer)).

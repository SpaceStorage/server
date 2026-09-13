# Contract: Per-Container Encryption

**Feature**: `003-type-system` | Crate: `crates/crypto` | Spec: FR-027, FR-027a, FR-028, SC-011 | Clarification Q5 | Constitution XIII

## 1. Setting

Per container: an **algorithm**, a **key reference**, and a **scope**.

| Algorithm | Crate | Key | Notes |
|---|---|---|---|
| `aes-256-gcm` | `aes-gcm` (RustCrypto) | 256-bit | default |
| `chacha20-poly1305` | `chacha20poly1305` (RustCrypto) | 256-bit | for hosts without AES acceleration |

Registry is open (FR-046); both are pure Rust (Principle I).

## 2. Scope (Clarification Q5)

| Scope | Covers | Default |
|---|---|---|
| `drives` | everything the container writes to drives: SSTable blocks, WAL records, segments, index files | **yes** |
| `drives_and_memory` | the above **plus** memory-resident stored data: memtable entries, block-cache entries, and the entire content of a memory-mode container | no |

Rules:

- Default is `drives`; memory-resident data may then be plaintext (FR-027a).
- A **memory-mode** container that declares encryption **must** declare `drives_and_memory`; `drives` there is a silent no-op and is refused with `EncryptionScopeInvalid{mode, scope}` naming the required scope.
- The scope in effect appears in the container description; `drives` shown as a default when not stated.

### The transient-buffer exemption (normative)

Under `drives_and_memory`, exactly these may hold plaintext, and only for the duration of one in-flight operation:

1. the decrypted plaintext of the blocks that operation touches;
2. the values assembled into the response being serialised;
3. the encoder's working buffer while producing a new block.

All three are zeroized on drop before the operation completes. **Nothing else** — no cache, no memtable, no index payload, no log line — may hold plaintext container data under this scope. SC-011 tests scan memory-resident regions for known plaintext markers outside these buffers.

### Scope change

- `drives → drives_and_memory`: takes effect as memory-resident data is repopulated (memtable flush cycle, cache refill, replica repopulation); the description shows requested vs fully-in-effect.
- `drives_and_memory → drives`: immediate, recorded.

## 3. Key handling

```rust
pub trait KeyAuthority: Send + Sync {                     // 07 implements this
    async fn resolve(&self, key_ref: &KeyRef) -> Result<KeyVersionSet, KeyError>;
    fn watch(&self, key_ref: &KeyRef) -> KeyWatch;        // rotation notifications
}
```

- A `KeyRef` is an opaque reference (e.g. `kv-rambler/data/ss/tenant-a`), never inline material (Constitution: certs and keys are referenced, never inlined).
- Per-container, per-version data keys are derived with **HKDF-SHA-256** from the referenced key material, salted with the `ContainerId`. Rotation adds a version; existing blocks stay readable under their recorded `key_version`, so FR-028 holds without rewriting data.
- Material lives only inside `crates/crypto` as `Zeroizing<[u8; 32]>` with no `Debug`, `Display`, `Serialize` or `Clone`-to-`Vec` path. It never enters a descriptor, container record, description, log line, admin response, CLI output or configuration dump (SC-011).
- **Unresolvable key**: at creation ⇒ `KeyUnresolvable{key_ref}`, nothing created, no data ever written unencrypted for that container. At restart ⇒ container present in the catalog with `state = Unavailable{KeyUnresolvable}`, every operation refused, no plaintext access, nothing discarded (FR-027, spec edge case).

### Interim provider (until `07`)

`keys { keyring_file /etc/spacestorage/keyring; }` — a `0600` file of `name  base64(32-byte key)  [version=N]` lines, live re-read, validated at startup (`keyring_unreadable`, `keyring_permissions`, `keyring_syntax{line}`, `keyring_duplicate{name}`). `KeyAuthority` is the single trait `07` replaces; the file format is not a compatibility promise.

## 4. Pipeline position

Encryption is the **last** stage: encode → compress → encrypt ([codecs.md](codecs.md), [storage-layout.md](storage-layout.md) §2). Per-block AEAD with a random 96-bit nonce and associated data `container_id ‖ file_id ‖ block_index`. Nonce reuse is bounded by a per-key-version block budget; exhausting it forces a key-version bump rather than reuse.

## 5. Independence

Encryption is independent of encoding and compression (FR-024): any algorithm with any codec with any encoding, subject only to data-kind applicability. Two containers in one namespace may use different algorithms, different keys and different scopes (FR-027, Story 3 scenario 6).

## 6. Test obligations

- Round-trip per algorithm across every codec and mode (SC-004).
- Wrong-key read yields an authentication failure, never plaintext or a CRC-passing garbage block (the CRC is over the pre-encryption payload precisely so the two are distinguishable).
- Tamper test: flipping a byte in a block, or moving a block to another offset/container, fails AEAD verification.
- Rotation: write under v1, rotate, write under v2, read both.
- `SC-011` scans: no key material in any output; no plaintext in memory-resident regions under `drives_and_memory` outside the three exempt buffers.

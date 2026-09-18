# Contract: Envelope keys (`KeyAuthority`)

**Feature**: `014-authz-keys` | Crate: `crypto` | Spec: FR-006–FR-011 | Research R5 | Seam: [`003` encryption.md](../../003-type-system/contracts/encryption.md)

## Replace interim keyring

`keys { keyring_file P; }` → `KeyringRemoved`. Use:

```nginx
keys {
    master_key_file /etc/spacestorage/master.key;   # 32 bytes, 0600
    # create_master_if_absent;                      # laptop only; omitted in production starters
}
```

Missing file when any container is encrypted, or when the directive is present without `create_master_if_absent` → `MasterKeyRequired`. Mode not `0600` → `MasterKeyPermissions`. Inline material → `KeyMaterialForbidden`.

## Hierarchy

1. **Master** — file. Backup = copy the file. Lost master without backup ⇒ encrypted containers unrestorable (explicit error). Unencrypted containers unaffected.
2. **Namespace KEK** — random 32 bytes, AES-256-GCM-wrapped with master, `KekRecord` in the cluster log. Created with the namespace.
3. **Data key** — random 32 bytes per `key_ref`, wrapped with the namespace KEK, stored on the container definition (`003`). `resolve(key_ref)` unwraps to `Zeroizing<[u8;32]>`. `003` still HKDF-SHA-256 with `ContainerId` for per-block keys.

Cache unwrapped data keys only for containers this node **hosts**. `CLUSTER_ADMIN` MAY unwrap for restore/admin. Not enclave/confidential computing.

## Bind

`CLUSTER_ADMIN` may bind a `key_ref` on any namespace (first binary). `NAMESPACE_ADMIN` bind is slice 7, that namespace only. Algorithms: `aes-256-gcm` (default), `chacha20-poly1305` (`003`).

## Rotate master

Rewrap every `KekRecord`. Data keys unchanged. 0 table rewrites (SC-005). Documented CLI: `spacestorage keys rotate-master --new-file P`.

## Rotate data key

Issue a new version on the container; old versions stay readable. Rewriting payloads is `010` (`DataKeyRewriteNotFirstBinary` if invoked on the first-binary profile).

## Restore

`013` restore `--key REF` and/or `--master-key-file`. Wrong/missing → fail naming the reference. Running node that has unwrapped keys can read hosted ciphertext.

## Who implements `KeyAuthority`

`EnvelopeAuthority` in `crypto`. `003` interim file provider is deleted once this crate is wired (`release-profile` first binary).

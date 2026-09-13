# Protocol Contract: `webdav`

**Crate**: `crates/handler-webdav` | **Stack**: `axum`/`hyper` 1.x (custom methods), `quick-xml`, `md-5` (Digest) | Research: R8

| Item | Value |
|------|-------|
| Wire version | HTTP/1.1 (keep-alive), HTTP/2 over TLS; **WebDAV class 1 and 2** (RFC 4918) — `OPTIONS` returns `DAV: 1, 2`, `Allow: OPTIONS, GET, HEAD, PUT, DELETE, PROPFIND, PROPPATCH, MKCOL, COPY, MOVE, LOCK, UNLOCK, REPORT`, `MS-Author-Via: DAV` |
| Signature (FR-004) | HTTP request line within 1 s; otherwise `400` |
| Authentication | `Basic` (recommended over TLS) and `Digest` (RFC 2617/7616 MD5, `qop=auth`, realm `protocols.webdav.realm`) → `Authenticator::Password/DigestMd5`; unauthenticated → `401` with both `WWW-Authenticate` challenges |
| Namespace / schema | credential-bound (FR-009a); first path segment = container in schema `public`; `/` is the namespace root (a synthetic collection listing containers) |
| Session identity | `User-Agent` → `client_app` |
| Type option | `MKCOL /{container}` headers `X-SpaceStorage-Type: <type>`, `X-SpaceStorage-Opt-<Name>: <value>`; default `object_collection`; extended `MKCOL` body (RFC 5689) with `<spacestorage:type>` also accepted |
| Listing (FR-013) | `PROPFIND /` `Depth: 1` lists every container as a collection with live property `{urn:spacestorage}type` (and `{urn:spacestorage}created-via`); `PROPFIND /{container}` lists objects |
| Native mappings | `object_collection` ↔ resources (body, `getcontenttype`, `getcontentlength`, `getlastmodified`, `getetag`), intermediate collections synthesised from `/`-separated key prefixes (`MKCOL` inside a container creates a zero-length marker object `<prefix>/` so empty folders persist); `kv_collection` ↔ resources with key as path and value as body |
| Canonical carrier | entity body with `Content-Type: application/vnd.spacestorage.canonical+json; v=1` on `PUT`/`GET` for any type |
| Type ops | `REPORT /{container}` with body `<spacestorage:op xmlns:spacestorage="urn:spacestorage" name="knn"><spacestorage:args>{canonical op-args}</spacestorage:args></spacestorage:op>` → `207`/`200` with `<spacestorage:result>` canonical `op-result` |
| Options | headers `X-SpaceStorage-Quorum`, `X-SpaceStorage-Timeout`, `X-SpaceStorage-Session`; response headers `X-SpaceStorage-Applied-*`, `X-SpaceStorage-Options-Source` |
| Methods (v1) | `OPTIONS`, `GET` (with `Range`, `If-*`), `HEAD`, `PUT` (`If-Match`, `If-None-Match`, `If:` lock tokens, chunked bodies, `Expect: 100-continue`), `DELETE` (resource or collection, `Depth: infinity`; top-level container delete = `Ddl::DropContainer`, `409` if the container type does not allow it), `PROPFIND` (`Depth: 0|1|infinity` (infinity capped, `403 propfind-finite-depth` when > 10 000 entries), `allprop`, `propname`, `prop`), `PROPPATCH` (dead properties stored in object `user_meta` under `dav:<ns>:<name>`; live properties protected → `403`), `MKCOL`, `COPY` (`Depth`, `Overwrite`), `MOVE` (`Overwrite`, cross-container allowed within the namespace), `LOCK` (exclusive/shared write locks, `Timeout:` (default `Second-3600`, max `Second-604800`), refresh via `If:` with lock token, `Depth: 0|infinity`, owner XML stored), `UNLOCK`, `REPORT` (SpaceStorage ops only; other reports → `403 supported-report`) |
| Locks | stored in the namespace's system container `.webdav_locks` (`ordered_map`, hidden from listings) so any node honours them (FR-032); tokens `urn:uuid:<v4>`; expired locks purged lazily; `If:` header parsed for tagged/untagged lists, `Not`, ETags |
| Live properties | `displayname getcontentlength getcontenttype getlastmodified getetag creationdate resourcetype supportedlock lockdiscovery quota-available-bytes quota-used-bytes (until 07: -1 / used bytes) {urn:spacestorage}type {urn:spacestorage}created-via` |
| Not supported (v1) | ACL (RFC 3744), versioning/DeltaV, `SEARCH` (RFC 5323), CalDAV/CardDAV, `BIND` → `501` or `403` per RFC with the method/report named |
| Error forms | RFC 4918 status codes; multi-status `207` for `PROPFIND/PROPPATCH/COPY/MOVE/DELETE` partial failures with per-resource `<D:status>`; precondition/postcondition XML (`<D:error><D:lock-token-submitted/></D:error>`, `<D:propfind-finite-depth/>`); `X-SpaceStorage-Error: timeout|quorum_unsatisfiable|not_supported|type_mismatch` on `504/503/501/400` |
| Name escaping (FR-019) | path segments percent-encoded per RFC 3986; `/` inside a container name is impossible by `ContainerName` rules; names beginning with `.` are hidden system containers |
| Stock clients (SC-001) | `cadaver`, `curl`, `davfs2`, macOS Finder, Windows Explorer (Mini-Redirector; requires `LOCK` and Basic over TLS), `rclone webdav`, Cyberduck, Nextcloud desktop, `reqwest_dav` (Rust) |
| Metrics | protocol label `webdav`; `kind` = method lower-case |

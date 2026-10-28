//! Container / type catalogs with CRUD for first-binary L3 models.

use crate::definition::{ContainerDefinition, StorageModeChoice};
use crate::descriptor::{ProductVersion, TypeDescriptor};
use spacestorage_compat::refuse_type_too_new;
use crate::error::TypeError;
use crate::ident::{new_container_id, ContainerId, ContainerName, NamespaceName};
use crate::schema::ContainerSchema;
use crate::validate::{validate_definition, validate_schema_alter};
use crate::{Container, L3Model};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug)]
pub struct TypeCatalog {
    types: HashMap<String, TypeDescriptor>,
}

impl Default for TypeCatalog {
    fn default() -> Self {
        Self::first_binary()
    }
}

impl TypeCatalog {
    pub fn empty() -> Self {
        Self {
            types: HashMap::new(),
        }
    }

    pub fn first_binary() -> Self {
        let mut t = Self::empty();
        for m in L3Model::first_binary_creatable() {
            t.types.insert(
                m.machine_name().into(),
                TypeDescriptor::l3_model(m.machine_name(), m.display_name(), m.schema_required()),
            );
        }
        t
    }

    /// Complete-product catalog: first-binary L3 + creatable L0 + L4 compositions.
    pub fn complete_product() -> Self {
        let mut t = Self::first_binary();
        for name in crate::l0::L0_CREATABLE {
            if let Some(d) = crate::l0::l0_descriptor(name) {
                t.types.insert((*name).into(), d);
            }
        }
        for name in crate::l0::L0_NOT_CREATABLE {
            if let Some(d) = crate::l0::l0_descriptor(name) {
                t.types.insert((*name).into(), d);
            }
        }
        for name in crate::composition::L4_COMPOSITIONS {
            if let Some(d) = crate::composition::l4_descriptor(name) {
                t.types.insert((*name).into(), d);
            }
        }
        t
    }

    pub fn get(&self, name: &str) -> Option<&TypeDescriptor> {
        self.types.get(name)
    }

    /// Resolve machine or display name against registered descriptors.
    pub fn resolve(&self, name: &str) -> Option<&TypeDescriptor> {
        if let Some(d) = self.types.get(name) {
            return Some(d);
        }
        let model = L3Model::parse(name)?;
        self.types.get(model.machine_name())
    }

    pub fn describe(&self, name: &str) -> Result<&TypeDescriptor, TypeError> {
        self.resolve(name).ok_or(TypeError::UnknownType)
    }

    /// Register or replace a descriptor (admin / tests).
    pub fn register_descriptor(&mut self, descriptor: TypeDescriptor) {
        self.types.insert(descriptor.name.clone(), descriptor);
    }
}

pub type Catalog = ContainerCatalog;

pub struct ContainerCatalog {
    pub types: TypeCatalog,
    /// Local cluster product version for rolling-upgrade refuse (015).
    product_version: ProductVersion,
    containers: HashMap<(String, String), Container>,
    /// In-memory row store for CRUD smoke (canonical blob path readiness).
    rows: HashMap<ContainerId, HashMap<String, Vec<u8>>>,
    /// Optional durable put (WAL) after memory insert — Persistent/Hybrid only.
    durable_put: Option<DurablePutHook>,
    /// Optional durable create (catalog definitions.jsonl).
    durable_create: Option<DurableCreateHook>,
    /// Optional 010 dual-write interceptor on the coordinating write path.
    write_interceptor: Option<WriteInterceptorHook>,
}

/// Sync hook invoked after an in-memory put for durable modes (013 residual).
pub type DurablePutHook =
    Arc<dyn Fn(ContainerId, StorageModeChoice, &str, &[u8]) -> Result<(), String> + Send + Sync>;

/// Sync hook invoked after container create for Persistent/Hybrid (definitions persist).
pub type DurableCreateHook = Arc<
    dyn Fn(ContainerId, &str, &str, L3Model, StorageModeChoice) -> Result<(), String> + Send + Sync,
>;

/// Coordinating write-path interceptor (010 dual-write). Called after the local row is
/// staged with `&mut Catalog` so the hook can mirror onto the target without re-locking;
/// `Err` rolls the source put back so the client is not acked.
pub type WriteInterceptorHook = Arc<
    dyn Fn(&mut ContainerCatalog, ContainerId, &str, &[u8]) -> Result<(), String> + Send + Sync,
>;

impl Default for ContainerCatalog {
    fn default() -> Self {
        Self::new()
    }
}

impl ContainerCatalog {
    pub fn new() -> Self {
        Self {
            types: TypeCatalog::first_binary(),
            product_version: ProductVersion::FIRST_BINARY,
            containers: HashMap::new(),
            rows: HashMap::new(),
            durable_put: None,
            durable_create: None,
            write_interceptor: None,
        }
    }

    pub fn product_version(&self) -> ProductVersion {
        self.product_version
    }

    pub fn set_product_version(&mut self, version: ProductVersion) {
        self.product_version = version;
    }

    pub fn set_durable_hooks(
        &mut self,
        put: Option<DurablePutHook>,
        create: Option<DurableCreateHook>,
    ) {
        self.durable_put = put;
        self.durable_create = create;
    }

    pub fn set_write_interceptor(&mut self, hook: Option<WriteInterceptorHook>) {
        self.write_interceptor = hook;
    }

    /// Create via `validate_definition` + `TypeCatalog` lookup (FR-016).
    pub fn create(
        &mut self,
        namespace: impl Into<String>,
        name: impl Into<String>,
        model: L3Model,
        multi_active: bool,
        schema: Option<ContainerSchema>,
        mode: StorageModeChoice,
    ) -> Result<ContainerId, TypeError> {
        let def = ContainerDefinition {
            type_name: model.machine_name().into(),
            mode,
            multi_active,
            schema: schema.clone(),
            encryption: None,
        };
        self.create_from_definition(namespace, name, &def)
    }

    pub fn create_from_definition(
        &mut self,
        namespace: impl Into<String>,
        name: impl Into<String>,
        def: &ContainerDefinition,
    ) -> Result<ContainerId, TypeError> {
        if matches!(def.mode, StorageModeChoice::Memory) {
            if let Some(enc) = &def.encryption {
                if matches!(enc.scope, crate::definition::EncryptionScope::Drives) {
                    return Err(TypeError::Msg(
                        "EncryptionScopeInvalid{mode=memory, scope=drives}".into(),
                    ));
                }
            }
        }
        let model = validate_definition(&self.types, def)?;
        let descriptor = self.types.describe(&def.type_name)?;
        refuse_type_too_new(
            self.product_version,
            &descriptor.name,
            descriptor.introduced_in,
        )
        .map_err(|e| TypeError::Msg(e.to_string()))?;
        let ns = namespace.into();
        let n = name.into();
        let key = (ns.clone(), n.clone());
        if self.containers.contains_key(&key) {
            return Err(TypeError::AlreadyExists);
        }
        let id = new_container_id();
        let c = Container {
            id,
            name: ContainerName::new(n.clone()),
            namespace: NamespaceName::new(ns.clone()),
            model,
            multi_active: false,
            schema: def.schema.clone(),
            mode: def.mode,
            encryption: def.encryption.clone(),
        };
        self.rows.insert(id, HashMap::new());
        self.containers.insert(key, c);
        if !matches!(def.mode, StorageModeChoice::Memory) {
            if let Some(hook) = &self.durable_create {
                hook(id, &ns, &n, model, def.mode).map_err(|e| TypeError::Msg(e))?;
            }
        }
        Ok(id)
    }

    /// Restore a container with a known id (boot from definitions.jsonl).
    pub fn restore_container(
        &mut self,
        id: ContainerId,
        namespace: impl Into<String>,
        name: impl Into<String>,
        model: L3Model,
        mode: StorageModeChoice,
        schema: Option<ContainerSchema>,
    ) -> Result<(), TypeError> {
        let ns = namespace.into();
        let n = name.into();
        let key = (ns.clone(), n.clone());
        if self.containers.contains_key(&key) {
            return Err(TypeError::AlreadyExists);
        }
        let c = Container {
            id,
            name: ContainerName::new(n),
            namespace: NamespaceName::new(ns),
            model,
            multi_active: false,
            schema,
            mode,
            encryption: None,
        };
        self.rows.entry(id).or_default();
        self.containers.insert(key, c);
        Ok(())
    }

    /// Replace in-memory rows for a container (WAL content hydrate).
    pub fn replace_rows(
        &mut self,
        id: ContainerId,
        rows: HashMap<String, Vec<u8>>,
    ) -> Result<(), TypeError> {
        if !self.rows.contains_key(&id) {
            return Err(TypeError::NotFound);
        }
        self.rows.insert(id, rows);
        Ok(())
    }

    /// First-binary alter: type fixed for life; additive schema / mode updates (FR-017b).
    pub fn alter(
        &mut self,
        namespace: &str,
        name: &str,
        new_type: Option<&str>,
        schema: Option<ContainerSchema>,
        mode: Option<StorageModeChoice>,
    ) -> Result<(), TypeError> {
        let key = (namespace.into(), name.into());
        let (id, prev_mode, prev_schema, prev_model) = {
            let c = self
                .containers
                .get(&key)
                .ok_or(TypeError::NotFound)?;
            (c.id, c.mode, c.schema.clone(), c.model)
        };

        if let Some(tn) = new_type {
            let requested = self
                .types
                .resolve(tn)
                .and_then(|d| L3Model::parse(&d.name))
                .or_else(|| L3Model::parse(tn));
            if requested.map(|m| m != prev_model).unwrap_or(true) {
                return Err(TypeError::TypeImmutable);
            }
        }

        if let Some(ref next_schema) = schema {
            validate_schema_alter(prev_schema.as_ref(), next_schema)?;
        }

        if let Some(m) = mode {
            if m != prev_mode {
                let has_rows = self.rows.get(&id).map(|r| !r.is_empty()).unwrap_or(false);
                if has_rows {
                    // Mode flip on existing rows = in-place codec/storage rewrite (FR-004).
                    return Err(crate::validate::refuse_inplace_codec_rewrite());
                }
            }
        }

        let c = self
            .containers
            .get_mut(&key)
            .ok_or(TypeError::NotFound)?;
        if let Some(next_schema) = schema {
            c.schema = Some(next_schema);
        }
        if let Some(m) = mode {
            c.mode = m;
        }

        Ok(())
    }

    /// Explicit refuse for in-place key rewrite of existing data (FR-004 / 010).
    pub fn refuse_inplace_key_change(&self) -> TypeError {
        crate::validate::refuse_inplace_key_rewrite()
    }

    pub fn describe(
        &self,
        namespace: &str,
        name: &str,
    ) -> Result<&Container, TypeError> {
        self.containers
            .get(&(namespace.into(), name.into()))
            .ok_or(TypeError::NotFound)
    }

    pub fn drop_container(&mut self, namespace: &str, name: &str) -> Result<(), TypeError> {
        let key = (namespace.into(), name.into());
        let c = self.containers.remove(&key).ok_or(TypeError::NotFound)?;
        self.rows.remove(&c.id);
        Ok(())
    }

    /// Remove a public name binding without deleting the container's rows (010 T078 retain_source).
    pub fn unbind_name(&mut self, namespace: &str, name: &str) -> Result<ContainerId, TypeError> {
        let key = (namespace.into(), name.into());
        let c = self.containers.remove(&key).ok_or(TypeError::NotFound)?;
        Ok(c.id)
    }

    /// Rebind a container to a new public name in the same namespace; rows stay on the id.
    pub fn rename_container(
        &mut self,
        namespace: &str,
        from: &str,
        to: &str,
    ) -> Result<(), TypeError> {
        let from_key = (namespace.into(), from.into());
        let to_key = (namespace.into(), to.into());
        if self.containers.contains_key(&to_key) {
            return Err(TypeError::AlreadyExists);
        }
        let mut c = self
            .containers
            .remove(&from_key)
            .ok_or(TypeError::NotFound)?;
        c.name = ContainerName::new(to);
        self.containers.insert(to_key, c);
        Ok(())
    }

    pub fn put(&mut self, id: ContainerId, key: &str, value: Vec<u8>) -> Result<(), TypeError> {
        let mode = self
            .containers
            .values()
            .find(|c| c.id == id)
            .map(|c| c.mode)
            .ok_or(TypeError::NotFound)?;
        if !self.rows.contains_key(&id) {
            return Err(TypeError::NotFound);
        }
        let prev = self.rows.get(&id).and_then(|m| m.get(key).cloned());
        if !matches!(mode, StorageModeChoice::Memory) {
            if let Some(hook) = &self.durable_put {
                hook(id, mode, key, &value).map_err(TypeError::Msg)?;
            }
        }
        let map = self.rows.get_mut(&id).ok_or(TypeError::NotFound)?;
        map.insert(key.into(), value.clone());
        // Dual-write: counted source ack waits for mapped target apply (010 R4).
        // Temporarily take the hook so it can call `put_mirrored` on `&mut self`.
        let hook = self.write_interceptor.take();
        let intercept = match &hook {
            Some(h) => h(self, id, key, &value),
            None => Ok(()),
        };
        self.write_interceptor = hook;
        if let Err(e) = intercept {
            let map = self.rows.get_mut(&id).ok_or(TypeError::NotFound)?;
            match prev {
                Some(old) => {
                    map.insert(key.into(), old);
                }
                None => {
                    map.remove(key);
                }
            }
            return Err(TypeError::Msg(e));
        }
        Ok(())
    }

    /// Apply a mirrored dual-write put without re-entering the write interceptor.
    /// Runs the durable put hook (WAL / write-quorum wait) for Persistent/Hybrid.
    pub fn put_mirrored(
        &mut self,
        id: ContainerId,
        key: &str,
        value: Vec<u8>,
    ) -> Result<(), TypeError> {
        let mode = self
            .containers
            .values()
            .find(|c| c.id == id)
            .map(|c| c.mode)
            .ok_or(TypeError::NotFound)?;
        if !self.rows.contains_key(&id) {
            return Err(TypeError::NotFound);
        }
        if !matches!(mode, StorageModeChoice::Memory) {
            if let Some(hook) = &self.durable_put {
                hook(id, mode, key, &value).map_err(TypeError::Msg)?;
            }
        }
        let map = self.rows.get_mut(&id).ok_or(TypeError::NotFound)?;
        map.insert(key.into(), value);
        Ok(())
    }

    pub fn get(&self, id: ContainerId, key: &str) -> Result<Option<&[u8]>, TypeError> {
        let map = self.rows.get(&id).ok_or(TypeError::NotFound)?;
        Ok(map.get(key).map(|v| v.as_slice()))
    }

    pub fn delete_row(&mut self, id: ContainerId, key: &str) -> Result<bool, TypeError> {
        let map = self.rows.get_mut(&id).ok_or(TypeError::NotFound)?;
        Ok(map.remove(key).is_some())
    }

    pub fn exists(&self, id: ContainerId, key: &str) -> Result<bool, TypeError> {
        let map = self.rows.get(&id).ok_or(TypeError::NotFound)?;
        Ok(map.contains_key(key))
    }

    /// Sorted key listing for SCAN/EXISTS helpers.
    pub fn keys(&self, id: ContainerId) -> Result<Vec<String>, TypeError> {
        let map = self.rows.get(&id).ok_or(TypeError::NotFound)?;
        let mut keys: Vec<String> = map.keys().cloned().collect();
        keys.sort();
        Ok(keys)
    }

    /// Cursor-based SCAN over container keys. `match_pat` supports `*` wildcards.
    pub fn scan(
        &self,
        id: ContainerId,
        cursor: u64,
        count: usize,
        match_pat: Option<&str>,
    ) -> Result<(u64, Vec<String>), TypeError> {
        let keys = self.keys(id)?;
        let start = cursor as usize;
        if start >= keys.len() {
            return Ok((0, Vec::new()));
        }
        let count = count.max(1);
        let mut out = Vec::new();
        let mut i = start;
        while i < keys.len() && out.len() < count {
            let k = &keys[i];
            if match_pat.map(|p| glob_match(p, k)).unwrap_or(true) {
                out.push(k.clone());
            }
            i += 1;
        }
        let next = if i >= keys.len() { 0 } else { i as u64 };
        Ok((next, out))
    }

    /// Logical byte/object counts for quota seeding (RF does not multiply).
    pub fn logical_usage(&self, id: ContainerId) -> (u64, u64) {
        self.rows
            .get(&id)
            .map(|rows| {
                (
                    rows.values().map(|v| v.len() as u64).sum(),
                    rows.len() as u64,
                )
            })
            .unwrap_or((0, 0))
    }

    pub fn list(&self) -> impl Iterator<Item = &Container> {
        self.containers.values()
    }
}

fn glob_match(pat: &str, key: &str) -> bool {
    if !pat.contains('*') && !pat.contains('?') {
        return pat == key;
    }
    let mut pi = 0;
    let mut ki = 0;
    let pb = pat.as_bytes();
    let kb = key.as_bytes();
    let mut star_p = None;
    let mut star_k = None;
    while ki < kb.len() {
        if pi < pb.len() && (pb[pi] == b'?' || pb[pi] == kb[ki]) {
            pi += 1;
            ki += 1;
        } else if pi < pb.len() && pb[pi] == b'*' {
            star_p = Some(pi);
            star_k = Some(ki);
            pi += 1;
        } else if let (Some(sp), Some(sk)) = (star_p, star_k) {
            pi = sp + 1;
            ki = sk + 1;
            star_k = Some(ki);
        } else {
            return false;
        }
    }
    while pi < pb.len() && pb[pi] == b'*' {
        pi += 1;
    }
    pi == pb.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Field, ValueDomain};
    use uuid::Uuid;

    #[test]
    fn default_matches_first_binary_new() {
        let a = TypeCatalog::default();
        let b = TypeCatalog::first_binary();
        assert!(a.get("kv_store").is_some());
        assert!(b.get("kv_store").is_some());
        assert!(a.get("relational_table").is_some());
        assert!(a.get("document_store").is_some());

        let c = ContainerCatalog::default();
        let d = ContainerCatalog::new();
        assert!(c.types.get("kv_store").is_some());
        assert!(d.types.get("kv_store").is_some());
    }

    #[test]
    fn encryption_reference_is_attached_and_described_without_key_bytes() {
        let mut catalog = ContainerCatalog::new();
        let definition = ContainerDefinition {
            type_name: "kv_store".into(),
            mode: StorageModeChoice::Persistent,
            multi_active: false,
            schema: None,
            encryption: Some(crate::definition::EncryptionDeclaration {
                algorithm: crate::definition::EncryptionAlgorithm::Aes256Gcm,
                key_ref: "vault://tenant/acme/data".into(),
                scope: crate::definition::EncryptionScope::Drives,
            }),
        };
        catalog
            .create_from_definition("acme", "items", &definition)
            .unwrap();
        let described = catalog.describe("acme", "items").unwrap();
        let json = serde_json::to_value(described.encryption.as_ref().unwrap()).unwrap();
        assert_eq!(json["key_ref"], "vault://tenant/acme/data");
        assert!(!json.to_string().contains("key_bytes"));
        assert!(!json.to_string().contains("material"));
    }

    #[test]
    fn memory_mode_refuses_encryption_scope_drives() {
        let mut catalog = ContainerCatalog::new();
        let definition = ContainerDefinition {
            type_name: "kv_store".into(),
            mode: StorageModeChoice::Memory,
            multi_active: false,
            schema: None,
            encryption: Some(crate::definition::EncryptionDeclaration {
                algorithm: crate::definition::EncryptionAlgorithm::Aes256Gcm,
                key_ref: "vault://tenant/acme/data".into(),
                scope: crate::definition::EncryptionScope::Drives,
            }),
        };
        let err = catalog
            .create_from_definition("acme", "items", &definition)
            .unwrap_err();
        assert!(err.to_string().contains("EncryptionScopeInvalid"));
    }

    #[test]
    fn create_refuses_unregistered_type_name() {
        let mut c = ContainerCatalog {
            types: TypeCatalog::empty(),
            product_version: ProductVersion::FIRST_BINARY,
            containers: HashMap::new(),
            rows: HashMap::new(),
            durable_put: None,
            durable_create: None,
            write_interceptor: None,
        };
        let def = ContainerDefinition {
            type_name: "kv_store".into(),
            mode: StorageModeChoice::Persistent,
            multi_active: false,
            schema: None,
            encryption: None,
        };
        assert!(matches!(
            c.create_from_definition("default", "x", &def),
            Err(TypeError::UnknownType)
        ));
    }

    #[test]
    fn alter_refuses_type_change_allows_additive_schema() {
        let mut c = ContainerCatalog::new();
        let schema = ContainerSchema {
            fields: vec![Field {
                name: "id".into(),
                domain: ValueDomain::Uuid,
                nullable: false,
            }],
        };
        c.create(
            "ns",
            "t",
            L3Model::RelationalTable,
            false,
            Some(schema),
            StorageModeChoice::Persistent,
        )
        .unwrap();

        assert!(matches!(
            c.alter("ns", "t", Some("kv_store"), None, None),
            Err(TypeError::TypeImmutable)
        ));

        let wider = ContainerSchema {
            fields: vec![
                Field {
                    name: "id".into(),
                    domain: ValueDomain::Uuid,
                    nullable: false,
                },
                Field {
                    name: "note".into(),
                    domain: ValueDomain::Utf8,
                    nullable: true,
                },
            ],
        };
        c.alter("ns", "t", None, Some(wider), Some(StorageModeChoice::Memory))
            .unwrap();
        let d = c.describe("ns", "t").unwrap();
        assert_eq!(d.schema.as_ref().unwrap().fields.len(), 2);
        assert_eq!(d.mode, StorageModeChoice::Memory);
        assert_eq!(d.model, L3Model::RelationalTable);
    }

    #[test]
    fn type_too_new_refused_on_create() {
        let mut c = ContainerCatalog::new();
        let mut desc = TypeDescriptor::l3_model("relational_table", "Relational Table", true);
        desc.introduced_in = ProductVersion(2);
        c.types.register_descriptor(desc);
        let schema = ContainerSchema {
            fields: vec![Field {
                name: "id".into(),
                domain: ValueDomain::Utf8,
                nullable: false,
            }],
        };
        let err = c
            .create(
                "ns",
                "t",
                L3Model::RelationalTable,
                false,
                Some(schema.clone()),
                StorageModeChoice::Memory,
            )
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("type_too_new")
                && err.contains("relational_table")
                && err.contains("local:1"),
            "{err}"
        );
        c.types
            .register_descriptor(TypeDescriptor::l3_model("relational_table", "Relational Table", true));
        assert!(
            c.create(
                "ns",
                "rt",
                L3Model::RelationalTable,
                false,
                Some(schema),
                StorageModeChoice::Memory,
            )
            .is_ok()
        );
    }

    #[test]
    fn container_ids_are_uuid_v7() {
        let mut c = ContainerCatalog::new();
        let id = c
            .create(
                "ns",
                "kv",
                L3Model::KvStore,
                false,
                None,
                StorageModeChoice::Persistent,
            )
            .unwrap();
        assert_eq!(id.get_version_num(), 7);
        assert_ne!(id, Uuid::nil());
    }

    #[test]
    fn rename_preserves_rows_drop_does_not() {
        let mut c = ContainerCatalog::new();
        let id = c
            .create(
                "ns",
                "t",
                L3Model::KvStore,
                false,
                None,
                StorageModeChoice::Persistent,
            )
            .unwrap();
        c.put(id, "k", b"v".to_vec()).unwrap();
        c.rename_container("ns", "t", "t__pre_transform_x")
            .unwrap();
        assert!(c.describe("ns", "t").is_err());
        assert_eq!(c.get(id, "k").unwrap(), Some(b"v".as_slice()));
        let id2 = c
            .create(
                "ns",
                "other",
                L3Model::KvStore,
                false,
                None,
                StorageModeChoice::Persistent,
            )
            .unwrap();
        c.put(id2, "a", b"1".to_vec()).unwrap();
        c.drop_container("ns", "other").unwrap();
        assert!(c.get(id2, "a").is_err());
    }
}

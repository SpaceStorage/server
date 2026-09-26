//! Container / type catalogs with CRUD for first-binary L3 models.

use crate::definition::{ContainerDefinition, StorageModeChoice};
use crate::descriptor::TypeDescriptor;
use crate::error::TypeError;
use crate::ident::{new_container_id, ContainerId, ContainerName, NamespaceName};
use crate::schema::ContainerSchema;
use crate::validate::{validate_definition, validate_schema_alter};
use crate::{Container, L3Model};
use std::collections::HashMap;

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
}

pub type Catalog = ContainerCatalog;

#[derive(Debug)]
pub struct ContainerCatalog {
    pub types: TypeCatalog,
    containers: HashMap<(String, String), Container>,
    /// In-memory row store for CRUD smoke (canonical blob path readiness).
    rows: HashMap<ContainerId, HashMap<String, Vec<u8>>>,
}

impl Default for ContainerCatalog {
    fn default() -> Self {
        Self::new()
    }
}

impl ContainerCatalog {
    pub fn new() -> Self {
        Self {
            types: TypeCatalog::first_binary(),
            containers: HashMap::new(),
            rows: HashMap::new(),
        }
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
        };
        self.create_from_definition(namespace, name, &def)
    }

    pub fn create_from_definition(
        &mut self,
        namespace: impl Into<String>,
        name: impl Into<String>,
        def: &ContainerDefinition,
    ) -> Result<ContainerId, TypeError> {
        let model = validate_definition(&self.types, def)?;
        let ns = namespace.into();
        let n = name.into();
        let key = (ns.clone(), n.clone());
        if self.containers.contains_key(&key) {
            return Err(TypeError::AlreadyExists);
        }
        let id = new_container_id();
        let c = Container {
            id,
            name: ContainerName::new(n),
            namespace: NamespaceName::new(ns),
            model,
            multi_active: false,
            schema: def.schema.clone(),
            mode: def.mode,
        };
        self.rows.insert(id, HashMap::new());
        self.containers.insert(key, c);
        Ok(id)
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
        let c = self
            .containers
            .get_mut(&key)
            .ok_or(TypeError::NotFound)?;

        if let Some(tn) = new_type {
            let requested = self
                .types
                .resolve(tn)
                .and_then(|d| L3Model::parse(&d.name))
                .or_else(|| L3Model::parse(tn));
            if requested.map(|m| m != c.model).unwrap_or(true) {
                return Err(TypeError::TypeImmutable);
            }
        }

        if let Some(next_schema) = schema {
            validate_schema_alter(c.schema.as_ref(), &next_schema)?;
            c.schema = Some(next_schema);
        }

        if let Some(m) = mode {
            c.mode = m;
        }

        Ok(())
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

    pub fn put(&mut self, id: ContainerId, key: &str, value: Vec<u8>) -> Result<(), TypeError> {
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
    fn create_refuses_unregistered_type_name() {
        let mut c = ContainerCatalog {
            types: TypeCatalog::empty(),
            containers: HashMap::new(),
            rows: HashMap::new(),
        };
        let def = ContainerDefinition {
            type_name: "kv_store".into(),
            mode: StorageModeChoice::Persistent,
            multi_active: false,
            schema: None,
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
}

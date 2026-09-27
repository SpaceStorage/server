//! L3 type catalog — first-binary creatable models (003 MVP slice).
//!
//! Full L0–L4 inventory is deferred; this crate ships K/V Store, Relational
//! Table, and Document Store with CRUD + `multi_active` refusal (T099 / US1).

pub mod ack;
pub mod catalog;
pub mod definition;
pub mod descriptor;
pub mod error;
pub mod ident;
pub mod schema;
pub mod validate;

pub use ack::{AckKind, WriteAck};
pub use catalog::{
    Catalog, ContainerCatalog, DurableCreateHook, DurablePutHook, TypeCatalog,
};
pub use definition::{ContainerDefinition, StorageModeChoice};
pub use descriptor::{Kind, Level, TypeDescriptor};
pub use error::TypeError;
pub use ident::{new_container_id, ContainerId, ContainerName, NamespaceName};
pub use schema::{ContainerSchema, Field, ValueDomain};
pub use validate::{validate_definition, validate_schema_alter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum L3Model {
    KvStore,
    RelationalTable,
    DocumentStore,
}

impl L3Model {
    pub fn machine_name(self) -> &'static str {
        match self {
            Self::KvStore => "kv_store",
            Self::RelationalTable => "relational_table",
            Self::DocumentStore => "document_store",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::KvStore => "K/V Store",
            Self::RelationalTable => "Relational Table",
            Self::DocumentStore => "Document Store",
        }
    }

    pub fn schema_required(self) -> bool {
        matches!(self, Self::RelationalTable)
    }

    pub fn first_binary_creatable() -> &'static [L3Model] {
        &[Self::KvStore, Self::RelationalTable, Self::DocumentStore]
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "kv_store" | "K/V Store" => Some(Self::KvStore),
            "relational_table" | "Relational Table" => Some(Self::RelationalTable),
            "document_store" | "Document Store" => Some(Self::DocumentStore),
            _ => None,
        }
    }

    /// Ordered / log-like types force multi_active off (FR-024b).
    pub fn multi_active_allowed(self) -> bool {
        // First-binary: all three refuse multi_active=on.
        false
    }
}

#[derive(Debug, Clone)]
pub struct Container {
    pub id: ContainerId,
    pub name: ContainerName,
    pub namespace: NamespaceName,
    pub model: L3Model,
    pub multi_active: bool,
    pub schema: Option<ContainerSchema>,
    pub mode: StorageModeChoice,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(model: L3Model, name: &str, schema: Option<ContainerSchema>) {
        let mut c = Catalog::new();
        let id = c
            .create(
                "default",
                name,
                model,
                false,
                schema,
                StorageModeChoice::Persistent,
            )
            .unwrap();
        let d = c.describe("default", name).unwrap();
        assert_eq!(d.id, id);
        assert_eq!(d.model, model);

        c.put(id, "k1", b"v1".to_vec()).unwrap();
        assert_eq!(c.get(id, "k1").unwrap(), Some(b"v1".as_slice()));
        assert!(c.delete_row(id, "k1").unwrap());
        assert_eq!(c.get(id, "k1").unwrap(), None);

        c.drop_container("default", name).unwrap();
        assert!(matches!(
            c.describe("default", name),
            Err(TypeError::NotFound)
        ));
    }

    #[test]
    fn create_describe_crud_drop_three_l3_models() {
        roundtrip(L3Model::KvStore, "kv", None);
        roundtrip(
            L3Model::RelationalTable,
            "rel",
            Some(ContainerSchema {
                fields: vec![Field {
                    name: "id".into(),
                    domain: ValueDomain::Uuid,
                    nullable: false,
                }],
            }),
        );
        roundtrip(L3Model::DocumentStore, "docs", None);
    }

    #[test]
    fn three_l3_creatable_and_multi_active_refused() {
        assert_eq!(L3Model::first_binary_creatable().len(), 3);
        let mut c = Catalog::default();
        c.create(
            "default",
            "kv",
            L3Model::KvStore,
            false,
            None,
            StorageModeChoice::Persistent,
        )
        .unwrap();
        let schema = ContainerSchema {
            fields: vec![Field {
                name: "id".into(),
                domain: ValueDomain::Uuid,
                nullable: false,
            }],
        };
        c.create(
            "default",
            "t",
            L3Model::RelationalTable,
            false,
            Some(schema),
            StorageModeChoice::Persistent,
        )
        .unwrap();
        c.create(
            "default",
            "docs",
            L3Model::DocumentStore,
            false,
            None,
            StorageModeChoice::Persistent,
        )
        .unwrap();
        assert!(matches!(
            c.create(
                "default",
                "x",
                L3Model::KvStore,
                true,
                None,
                StorageModeChoice::Persistent,
            ),
            Err(TypeError::MultiActiveUnsupported)
        ));
        assert!(matches!(
            c.create(
                "default",
                "bad",
                L3Model::RelationalTable,
                false,
                None,
                StorageModeChoice::Persistent,
            ),
            Err(TypeError::SchemaRequired)
        ));
    }
}

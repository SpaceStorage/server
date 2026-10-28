//! Catalog helpers shared by admin and protocol namespace DDL.

use spacestorage_types::{ContainerCatalog, ContainerId};
use std::sync::RwLock;

pub fn container_ids_for_namespace(
    catalog: &RwLock<ContainerCatalog>,
    name: &str,
) -> Vec<ContainerId> {
    catalog
        .read()
        .expect("catalog")
        .list()
        .filter(|container| container.namespace.as_str() == name)
        .map(|container| container.id)
        .collect()
}

/// When `cascade` is false and the namespace still has containers, return the
/// protocol/admin error string (`CascadeRequired{...}`).
pub fn namespace_delete_cascade_required(
    name: &str,
    cascade: bool,
    catalog: &RwLock<ContainerCatalog>,
) -> Option<String> {
    if cascade {
        return None;
    }
    let containers = container_ids_for_namespace(catalog, name);
    if containers.is_empty() {
        return None;
    }
    Some(format!(
        "CascadeRequired{{name={name}, containers={containers:?}}}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use spacestorage_types::{L3Model, StorageModeChoice};

    #[test]
    fn cascade_required_when_containers_remain() {
        let catalog = RwLock::new(ContainerCatalog::new());
        catalog
            .write()
            .unwrap()
            .create(
                "ns1",
                "tbl",
                L3Model::KvStore,
                false,
                None,
                StorageModeChoice::Memory,
            )
            .unwrap();
        let msg = namespace_delete_cascade_required("ns1", false, &catalog).unwrap();
        assert!(msg.contains("CascadeRequired"));
        assert!(msg.contains("ns1"));
        assert!(namespace_delete_cascade_required("ns1", true, &catalog).is_none());
        assert!(namespace_delete_cascade_required("empty", false, &catalog).is_none());
    }
}

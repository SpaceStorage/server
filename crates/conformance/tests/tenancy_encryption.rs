//! `007` encryption declaration describe (SC-007).

use spacestorage_admin_proto::AdminOp;
use spacestorage_conformance::{LAB_ADMIN_TOKEN, admin_tcp_json, boot_tenancy_one_node};
use spacestorage_types::{
    ContainerDefinition, EncryptionAlgorithm, EncryptionScope, StorageModeChoice,
};
use tempfile::tempdir;

#[test]
fn inline_key_material_is_forbidden_at_validate() {
    // Fixture `encryption-key-inline.conf` documents inline `key "…"`; enforcement is on declarations.
    let inline_key = "a".repeat(64);
    let err = spacestorage_tenancy::encryption::EncryptionDeclaration::validate(
        "AES-256-GCM",
        &inline_key,
        "drives",
        "persistent",
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("KeyMaterialForbidden"),
        "{err}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn describe_shows_encryption_reference_without_key_bytes() {
    let dir = tempdir().unwrap();
    let lab = boot_tenancy_one_node(dir.path()).await;
    {
        let mut cat = lab.node.catalog.write().unwrap();
        cat.create_from_definition(
            "acme",
            "items",
            &ContainerDefinition {
                type_name: "kv_store".into(),
                mode: StorageModeChoice::Persistent,
                multi_active: false,
                schema: None,
                encryption: Some(spacestorage_types::EncryptionDeclaration {
                    algorithm: EncryptionAlgorithm::Aes256Gcm,
                    key_ref: "vault://tenant/acme/data".into(),
                    scope: EncryptionScope::Drives,
                }),
            },
        )
        .unwrap();
    }

    let body = admin_tcp_json(
        lab.ports.admin,
        LAB_ADMIN_TOKEN,
        AdminOp::NamespaceDescribe {
            name: "acme".into(),
        },
    )
    .await;
    let text = body.to_string();
    assert!(text.contains("vault://tenant/acme/data"));
    assert!(!text.contains("key_bytes"));
    assert!(!text.to_lowercase().contains("material"));
    lab.shutdown().await;
}

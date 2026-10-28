use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StorageModeChoice {
    Memory,
    Persistent,
    Hybrid,
}

impl Default for StorageModeChoice {
    fn default() -> Self {
        Self::Persistent
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EncryptionAlgorithm {
    Aes256Gcm,
    ChaCha20Poly1305,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EncryptionScope {
    Drives,
    DrivesAndMemory,
}

/// Container metadata contains a key reference only; there is deliberately no key-bytes field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptionDeclaration {
    pub algorithm: EncryptionAlgorithm,
    pub key_ref: String,
    pub scope: EncryptionScope,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerDefinition {
    pub type_name: String,
    pub mode: StorageModeChoice,
    pub multi_active: bool,
    pub schema: Option<crate::schema::ContainerSchema>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encryption: Option<EncryptionDeclaration>,
}

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

#[derive(Debug, Clone)]
pub struct ContainerDefinition {
    pub type_name: String,
    pub mode: StorageModeChoice,
    pub multi_active: bool,
    pub schema: Option<crate::schema::ContainerSchema>,
}

//! Type level / kind / descriptor (003 foundational subset).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    L0,
    L1,
    L2,
    L3,
    L4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    DataStructure,
    StoragePrimitive,
    StorageLayout,
    SharedCapability,
    Abstraction,
    StorageModel,
    Composition,
}

impl Kind {
    pub fn creatable(self) -> bool {
        matches!(
            self,
            Self::DataStructure | Self::Abstraction | Self::StorageModel | Self::Composition
        )
    }
}

#[derive(Debug, Clone)]
pub struct TypeDescriptor {
    pub name: String,
    pub display_name: String,
    pub level: Level,
    pub kind: Kind,
    /// FR-024b: default off; ordered/log types forced off.
    pub multi_active: bool,
    pub schema_required: bool,
}

impl TypeDescriptor {
    pub fn l3_model(name: &str, display: &str, schema_required: bool) -> Self {
        Self {
            name: name.into(),
            display_name: display.into(),
            level: Level::L3,
            kind: Kind::StorageModel,
            multi_active: false,
            schema_required,
        }
    }
}

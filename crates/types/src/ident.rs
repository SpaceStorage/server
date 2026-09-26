use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type ContainerId = Uuid;

/// Allocate a new container identity (UUID v7 — time-ordered, immutable for life).
pub fn new_container_id() -> ContainerId {
    Uuid::now_v7()
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NamespaceName(String);

impl NamespaceName {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContainerName(String);

impl ContainerName {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

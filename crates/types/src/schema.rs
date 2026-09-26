use crate::error::TypeError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValueDomain {
    Bool,
    Int32,
    Int64,
    Uint64,
    Float32,
    Float64,
    Utf8,
    Bytes,
    Uuid,
    Timestamp,
    Jsonb,
    Null,
}

impl ValueDomain {
    /// First-binary widening lattice subset (FR-017b).
    pub fn widens_to(self, other: Self) -> bool {
        if self == other {
            return true;
        }
        matches!(
            (self, other),
            (Self::Int32, Self::Int64) | (Self::Float32, Self::Float64)
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub domain: ValueDomain,
    pub nullable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerSchema {
    pub fields: Vec<Field>,
}

/// Accept additive / widening schema changes; refuse drop/rename/narrow (FR-017b).
pub fn is_additive_schema_change(
    from: &ContainerSchema,
    to: &ContainerSchema,
) -> Result<(), TypeError> {
    for prev in &from.fields {
        let Some(next) = to.fields.iter().find(|f| f.name == prev.name) else {
            return Err(TypeError::IncompatibleSchemaChange);
        };
        if !prev.domain.widens_to(next.domain.clone()) {
            return Err(TypeError::IncompatibleSchemaChange);
        }
        // nullable → non-null is a narrowing refuse
        if prev.nullable && !next.nullable {
            return Err(TypeError::IncompatibleSchemaChange);
        }
    }
    Ok(())
}

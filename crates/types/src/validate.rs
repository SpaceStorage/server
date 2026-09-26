use crate::catalog::TypeCatalog;
use crate::definition::ContainerDefinition;
use crate::error::TypeError;
use crate::schema::{is_additive_schema_change, ContainerSchema};
use crate::L3Model;

/// Validate a create definition against the registered type catalog (FR-016 / FR-018).
pub fn validate_definition(
    types: &TypeCatalog,
    def: &ContainerDefinition,
) -> Result<L3Model, TypeError> {
    let desc = types
        .resolve(&def.type_name)
        .ok_or(TypeError::UnknownType)?;
    if !desc.kind.creatable() {
        return Err(TypeError::NotCreatable);
    }
    let model = L3Model::parse(&desc.name).ok_or(TypeError::UnknownType)?;
    if def.multi_active {
        return Err(TypeError::MultiActiveUnsupported);
    }
    if (model.schema_required() || desc.schema_required)
        && def
            .schema
            .as_ref()
            .map(|s| s.fields.is_empty())
            .unwrap_or(true)
    {
        return Err(TypeError::SchemaRequired);
    }
    Ok(model)
}

/// Validate an alter that only updates schema (additive / widen — FR-017b).
pub fn validate_schema_alter(
    from: Option<&ContainerSchema>,
    to: &ContainerSchema,
) -> Result<(), TypeError> {
    match from {
        None => Ok(()),
        Some(prev) => is_additive_schema_change(prev, to),
    }
}

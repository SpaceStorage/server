use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TypeError {
    #[error("unknown type")]
    UnknownType,
    #[error("not creatable")]
    NotCreatable,
    #[error("already exists")]
    AlreadyExists,
    #[error("schema required")]
    SchemaRequired,
    #[error("multi_active unsupported")]
    MultiActiveUnsupported,
    #[error(
        "type immutable in place; use data_transformation job (010 rewrite=type_model)"
    )]
    TypeImmutable,
    #[error(
        "incompatible schema change refused in place; use data_transformation job (010 rewrite=incompatible_schema)"
    )]
    IncompatibleSchemaChange,
    /// In-place codec or key rewrite of existing data — use 010 transform instead.
    #[error(
        "in-place {0} of existing data refused; use data_transformation job (010)"
    )]
    InPlaceRewriteRefused(&'static str),
    #[error("not found")]
    NotFound,
    #[error("unsupported storage mode")]
    UnsupportedStorageMode,
    #[error("incomplete descriptor")]
    IncompleteDescriptor,
    #[error("{0}")]
    Msg(String),
}

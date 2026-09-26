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
    #[error("type immutable")]
    TypeImmutable,
    #[error("incompatible schema change")]
    IncompatibleSchemaChange,
    #[error("not found")]
    NotFound,
    #[error("unsupported storage mode")]
    UnsupportedStorageMode,
    #[error("incomplete descriptor")]
    IncompleteDescriptor,
}

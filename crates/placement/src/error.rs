use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PlacementError {
    #[error("PlacementUnsatisfiable")]
    PlacementUnsatisfiable {
        container: String,
        constraint: String,
        hint: String,
    },
    #[error("AntiAffinityKeyMissing")]
    AntiAffinityKeyMissing,
    #[error("InternodesRequired")]
    InternodesRequired,
    #[error("QuorumUnsatisfiable")]
    QuorumUnsatisfiable,
    #[error("MultiActiveRefused")]
    MultiActiveRefused,
    /// Fixed sharding-key change in place — use 010 transform rewrite=sharding_key.
    #[error(
        "fixed sharding-key change refused for {container}; use data_transformation job (010 rewrite=sharding_key)"
    )]
    FixedShardingKeyChange { container: String },
    #[error("{0}")]
    Msg(String),
}

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
    #[error("{0}")]
    Msg(String),
}

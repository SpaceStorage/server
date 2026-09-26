//! Promote / epoch fence.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FenceError {
    #[error("PromoteNotCaughtUp")]
    NotCaughtUp,
    #[error("PromoteNeedsDataLossAccept")]
    NeedsDataLossAccept,
    #[error("TwoSources")]
    TwoSources,
    #[error("EpochFenced")]
    EpochFenced,
}

pub fn ordinary_promote_allowed(caught_up: bool, old_source_unavailable: bool) -> Result<(), FenceError> {
    if caught_up || old_source_unavailable {
        Ok(())
    } else {
        Err(FenceError::NotCaughtUp)
    }
}

pub fn force_promote(accept_data_loss: bool) -> Result<(), FenceError> {
    if accept_data_loss {
        Ok(())
    } else {
        Err(FenceError::NeedsDataLossAccept)
    }
}

//! Quota checks at start and cutover (010 R11 / FR-011).

use crate::error::MigrateError;

#[derive(Debug, Clone, Copy, Default)]
pub struct QuotaView {
    pub logical_usage: u64,
    pub soft_limit: Option<u64>,
}

impl QuotaView {
    pub fn check_fits(&self, additional: u64) -> Result<(), MigrateError> {
        let Some(limit) = self.soft_limit else {
            return Ok(());
        };
        if self.logical_usage.saturating_add(additional) > limit {
            return Err(MigrateError::QuotaExceeded);
        }
        Ok(())
    }
}

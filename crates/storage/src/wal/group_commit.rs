//! Group-commit coalesce defaults.

use std::time::Duration;

#[derive(Debug, Clone)]
pub struct GroupCommitConfig {
    pub max_wait: Duration,
    pub max_bytes: usize,
}

impl Default for GroupCommitConfig {
    fn default() -> Self {
        Self {
            max_wait: Duration::from_millis(2),
            max_bytes: 1024 * 1024,
        }
    }
}

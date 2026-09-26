//! Reloadable tracing filter for live `log.level` (FR-013 / T086).

use std::sync::OnceLock;
use tracing::warn;
use tracing_subscriber::reload;
use tracing_subscriber::{EnvFilter, Registry};

pub type FilterReloadHandle = reload::Handle<EnvFilter, Registry>;

static LOG_RELOAD: OnceLock<FilterReloadHandle> = OnceLock::new();

/// Install the reload handle produced at subscriber init (daemon startup).
pub fn install_reload_handle(handle: FilterReloadHandle) {
    if LOG_RELOAD.set(handle).is_err() {
        warn!("log reload handle already installed; ignoring duplicate");
    }
}

/// Apply a validated `log.level` string to the active tracing subscriber.
pub fn apply_level(level: &str) {
    let Some(handle) = LOG_RELOAD.get() else {
        return;
    };
    let filter = match EnvFilter::try_new(level) {
        Ok(f) => f,
        Err(e) => {
            warn!(error=%e, level=%level, "invalid log.level for tracing filter");
            return;
        }
    };
    if let Err(e) = handle.reload(filter) {
        warn!(error=%e, level=%level, "failed to apply log.level");
    }
}

//! HLC stamps and domain clocks (012).

pub mod skew;
pub mod stamp;

pub use skew::SkewMonitor;
pub use stamp::{HlcCompareError, HlcStamp};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum ClockError {
    #[error("io: {0}")]
    Io(String),
    #[error(transparent)]
    Compare(#[from] HlcCompareError),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedClock {
    last: HlcStamp,
    persisted_at_micros: u64,
}

/// Per-domain HLC with persist path `{data_dir}/clocks/<domain>.json`.
pub struct DomainClock {
    domain_id: String,
    node_id: Uuid,
    path: PathBuf,
    inner: Mutex<HlcStamp>,
}

impl DomainClock {
    pub async fn load_or_new(
        data_dir: &Path,
        domain_id: impl Into<String>,
        node_id: Uuid,
    ) -> Result<Self, ClockError> {
        let domain_id = domain_id.into();
        let dir = data_dir.join("clocks");
        let path = dir.join(format!("{domain_id}.json"));
        let last = if path.exists() {
            let bytes = tokio::fs::read(&path)
                .await
                .map_err(|e| ClockError::Io(e.to_string()))?;
            let p: PersistedClock =
                serde_json::from_slice(&bytes).map_err(|e| ClockError::Io(e.to_string()))?;
            p.last
        } else {
            HlcStamp {
                domain_id: domain_id.clone(),
                physical_micros: 0,
                logical: 0,
                node_id,
            }
        };
        Ok(Self {
            domain_id,
            node_id,
            path,
            inner: Mutex::new(last),
        })
    }

    pub fn tick(&self, wall_micros: u64) -> HlcStamp {
        let mut g = self.inner.lock();
        *g = g.tick(wall_micros, self.node_id);
        g.clone()
    }

    pub fn observe(&self, remote: &HlcStamp) -> Result<(), HlcCompareError> {
        let mut g = self.inner.lock();
        *g = g.merge_observe(remote, self.node_id)?;
        Ok(())
    }

    pub async fn persist(&self) -> Result<(), ClockError> {
        let last = self.inner.lock().clone();
        let wall = stamp::wall_micros();
        let body = PersistedClock {
            last,
            persisted_at_micros: wall,
        };
        let bytes = serde_json::to_vec_pretty(&body).map_err(|e| ClockError::Io(e.to_string()))?;
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| ClockError::Io(e.to_string()))?;
            }
            std::fs::write(&path, &bytes).map_err(|e| ClockError::Io(e.to_string()))?;
            let f = std::fs::File::open(&path).map_err(|e| ClockError::Io(e.to_string()))?;
            f.sync_all().map_err(|e| ClockError::Io(e.to_string()))?;
            Ok::<(), ClockError>(())
        })
        .await
        .map_err(|e| ClockError::Io(e.to_string()))?
    }

    pub fn domain_id(&self) -> &str {
        &self.domain_id
    }
}

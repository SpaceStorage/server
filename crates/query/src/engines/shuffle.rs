//! Shuffle RPC kinds consumed over internode (005 FR-027/FR-029).

use crate::error::ExecError;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Additive message kind names (mirrored in `crates/internode` registry).
pub const SHUFFLE_OFFER: &str = "ShuffleOffer";
pub const SHUFFLE_PUSH: &str = "ShufflePush";
pub const SHUFFLE_PULL: &str = "ShufflePull";
pub const STAGE_ABORT: &str = "StageAbort";

#[derive(Debug, Clone)]
pub struct ShuffleJob {
    pub id: Uuid,
}

impl ShuffleJob {
    pub fn new() -> Self {
        Self {
            id: Uuid::now_v7(),
        }
    }
}

impl Default for ShuffleJob {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Default)]
struct PartStore {
    parts: HashMap<(Uuid, u32), Vec<Vec<u8>>>,
    aborted: HashMap<Uuid, String>,
}

/// In-process shuffle transport (internode wire uses same kind names).
#[derive(Debug, Clone, Default)]
pub struct ShuffleTransport {
    inner: Arc<Mutex<PartStore>>,
    fail_peer: Arc<Mutex<bool>>,
}

impl ShuffleTransport {
    pub fn new() -> Self {
        Self::default()
    }

    /// Test hook: ops fail as peer kill while armed.
    pub fn simulate_peer_kill(&self) {
        *self.fail_peer.lock() = true;
    }

    fn check_peer(&self) -> Result<(), ExecError> {
        if *self.fail_peer.lock() {
            Err(ExecError::StageFailed("shuffle".into()))
        } else {
            Ok(())
        }
    }

    pub fn offer(&self, job: &ShuffleJob, partition: u32) -> Result<(), ExecError> {
        self.check_peer()?;
        let mut st = self.inner.lock();
        st.parts.entry((job.id, partition)).or_default();
        Ok(())
    }

    pub fn push(&self, job: &ShuffleJob, partition: u32, bytes: &[u8]) -> Result<(), ExecError> {
        self.check_peer()?;
        let mut st = self.inner.lock();
        st.parts
            .entry((job.id, partition))
            .or_default()
            .push(bytes.to_vec());
        Ok(())
    }

    pub fn pull(&self, job: &ShuffleJob, partition: u32) -> Result<Vec<Vec<u8>>, ExecError> {
        self.check_peer()?;
        let st = self.inner.lock();
        Ok(st.parts.get(&(job.id, partition)).cloned().unwrap_or_default())
    }

    pub fn abort(&self, job: &ShuffleJob, stage: &str) {
        self.inner
            .lock()
            .aborted
            .insert(job.id, stage.to_string());
    }

    /// Retry until timeout; never hang past `deadline`.
    pub fn push_with_retry(
        &self,
        job: &ShuffleJob,
        partition: u32,
        bytes: &[u8],
        deadline: Duration,
    ) -> Result<(), ExecError> {
        let start = Instant::now();
        loop {
            match self.push(job, partition, bytes) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    if start.elapsed() >= deadline {
                        return Err(e);
                    }
                    // brief backoff without blocking worker: yield via std sleep in spawn_blocking
                    // callers; here we just loop with Instant check.
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_kill_fails_not_hang() {
        let t = ShuffleTransport::new();
        let job = ShuffleJob::new();
        t.offer(&job, 0).unwrap();
        t.simulate_peer_kill();
        let err = t
            .push_with_retry(&job, 0, b"x", Duration::from_millis(20))
            .unwrap_err();
        assert_eq!(err.code(), "stage_failed");
    }
}

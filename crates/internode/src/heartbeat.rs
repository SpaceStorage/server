//! Failure detector from heartbeat freshness.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerStatus {
    Alive,
    Unavailable,
}

#[derive(Debug)]
pub struct FailureDetectorView {
    failure_timeout: Duration,
    last: Mutex<HashMap<String, Instant>>,
}

impl FailureDetectorView {
    pub fn new(failure_timeout: Duration) -> Self {
        Self {
            failure_timeout,
            last: Mutex::new(HashMap::new()),
        }
    }

    pub fn note_heartbeat(&self, peer_id: impl Into<String>) {
        self.last.lock().insert(peer_id.into(), Instant::now());
    }

    pub fn status(&self, peer_id: &str, now: Instant) -> PeerStatus {
        match self.last.lock().get(peer_id) {
            Some(t) if now.duration_since(*t) < self.failure_timeout => PeerStatus::Alive,
            _ => PeerStatus::Unavailable,
        }
    }

    pub fn is_unavailable(&self, peer_id: &str) -> bool {
        self.status(peer_id, Instant::now()) == PeerStatus::Unavailable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_marks_unavailable() {
        let fd = FailureDetectorView::new(Duration::from_millis(50));
        fd.note_heartbeat("n2");
        assert_eq!(fd.status("n2", Instant::now()), PeerStatus::Alive);
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(fd.status("n2", Instant::now()), PeerStatus::Unavailable);
    }
}

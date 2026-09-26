//! HLC skew monitoring (max_stamp_skew default 500ms).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[derive(Debug)]
pub struct SkewMonitor {
    pub max_skew_micros: u64,
    pub last_skew_micros: AtomicU64,
    pub over_limit: AtomicBool,
}

impl SkewMonitor {
    pub fn new(max_skew_micros: u64) -> Self {
        Self {
            max_skew_micros,
            last_skew_micros: AtomicU64::new(0),
            over_limit: AtomicBool::new(false),
        }
    }

    pub fn default_500ms() -> Self {
        Self::new(500_000)
    }

    /// Sample |remote - local| physical; set over_limit when above threshold.
    pub fn sample(&self, local_physical: u64, remote_physical: u64) -> bool {
        let skew = local_physical.abs_diff(remote_physical);
        self.last_skew_micros.store(skew, Ordering::Relaxed);
        let over = skew > self.max_skew_micros;
        self.over_limit.store(over, Ordering::Relaxed);
        over
    }

    pub fn is_over_limit(&self) -> bool {
        self.over_limit.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn over_limit_degrades() {
        let m = SkewMonitor::new(500_000);
        assert!(!m.sample(1_000_000, 1_100_000));
        assert!(m.sample(1_000_000, 2_000_000));
        assert!(m.is_over_limit());
    }
}

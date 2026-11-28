//! Key/field expiry (`017` T009). The absolute deadline is an event-time (or
//! source-domain ingest/HLC) instant from the `013` TTL clock; this handler
//! mirrors it so a late expiration job cannot resurrect a value that a reader
//! already observes as missing (purge-on-read).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Wall-clock/event-time instant in Unix milliseconds — the `013` TTL clock unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TtlInstant {
    pub millis: i64,
}

impl TtlInstant {
    pub fn from_unix_millis(millis: i64) -> Self {
        Self { millis }
    }

    pub fn now() -> Self {
        Self {
            millis: now_unix_millis(),
        }
    }

    pub fn after(now: Self, d: Duration) -> Self {
        Self {
            millis: now.millis.saturating_add(d.as_millis() as i64),
        }
    }
}

pub fn now_unix_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// An absolute key or hash-field expiry (`RedisEntry.expire_at`,
/// `HashField.expire_at`). `None` on the owner means persistent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deadline {
    pub at: TtlInstant,
}

impl Deadline {
    pub fn new(at: TtlInstant) -> Self {
        Self { at }
    }

    /// Readers treat `at <= now` as missing (purge-on-read).
    pub fn is_past(&self, now: TtlInstant) -> bool {
        self.at <= now
    }
}

/// `TTL` / `PTTL` reply value: `-2` missing, `-1` no expiry, else remaining
/// lifetime in seconds (`millis == false`) or milliseconds (`millis == true`).
pub fn report_ttl(exists: bool, expire_at: Option<Deadline>, now: TtlInstant, millis: bool) -> i64 {
    if !exists {
        return -2;
    }
    match expire_at {
        None => -1,
        Some(d) if d.is_past(now) => -2,
        Some(d) => {
            let remaining = d.at.millis.saturating_sub(now.millis);
            if millis { remaining } else { remaining / 1000 }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ttl_reporting_no_expiry_and_missing() {
        let now = TtlInstant::from_unix_millis(1_000);
        assert_eq!(report_ttl(false, None, now, false), -2);
        assert_eq!(report_ttl(true, None, now, false), -1);
        assert_eq!(report_ttl(true, None, now, true), -1);
    }

    #[test]
    fn ttl_reporting_remaining_and_expired() {
        let now = TtlInstant::from_unix_millis(1_000);
        let d = Deadline::new(TtlInstant::from_unix_millis(61_000));
        assert_eq!(report_ttl(true, Some(d), now, false), 60);
        assert_eq!(report_ttl(true, Some(d), now, true), 60_000);
        let past = Deadline::new(TtlInstant::from_unix_millis(999));
        assert_eq!(report_ttl(true, Some(past), now, false), -2);
    }
}

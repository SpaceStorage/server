//! Replica / shuffle ranking (ladder → RTT → skew).

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RankKey {
    /// Index of first differing topology ladder key; missing = u8::MAX (farthest).
    pub ladder_distance: u8,
    pub rtt: Option<Duration>,
    pub skew_unhealthy: bool,
}

impl RankKey {
    pub fn missing_ladder() -> Self {
        Self {
            ladder_distance: u8::MAX,
            rtt: None,
            skew_unhealthy: false,
        }
    }
}

impl Ord for RankKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.ladder_distance
            .cmp(&other.ladder_distance)
            .then_with(|| match (&self.rtt, &other.rtt) {
                (Some(a), Some(b)) => a.cmp(b),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            })
            .then_with(|| self.skew_unhealthy.cmp(&other.skew_unhealthy))
    }
}

impl PartialOrd for RankKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Rank candidates; lower RankKey is preferred.
pub fn rank_nodes(mut keys: Vec<(String, RankKey)>) -> Vec<String> {
    keys.sort_by(|a, b| a.1.cmp(&b.1));
    keys.into_iter().map(|(n, _)| n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ladder_then_rtt_then_skew() {
        let a = RankKey {
            ladder_distance: 0,
            rtt: Some(Duration::from_millis(5)),
            skew_unhealthy: false,
        };
        let b = RankKey {
            ladder_distance: 0,
            rtt: Some(Duration::from_millis(10)),
            skew_unhealthy: false,
        };
        let c = RankKey {
            ladder_distance: 0,
            rtt: None,
            skew_unhealthy: false,
        };
        let healthy = RankKey {
            ladder_distance: 0,
            rtt: Some(Duration::from_millis(5)),
            skew_unhealthy: false,
        };
        let skew = RankKey {
            ladder_distance: 0,
            rtt: Some(Duration::from_millis(5)),
            skew_unhealthy: true,
        };
        let e = RankKey::missing_ladder();
        assert!(a < b);
        assert!(b < c);
        assert!(healthy < skew);
        assert!(a < e);
        let ranked = rank_nodes(vec![
            ("far".into(), e),
            ("slow".into(), b),
            ("fast".into(), a),
            ("skew".into(), skew),
        ]);
        assert_eq!(ranked[0], "fast");
        assert_eq!(ranked.last().unwrap(), "far");
    }
}

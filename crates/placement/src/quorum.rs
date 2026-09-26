//! Quorum vocabulary and product defaults (write TWO / read ONE; one-node ONE).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuorumLevel {
    One,
    LocalOne,
    Two,
    Quorum,
    All,
    Acks(u32),
}

impl QuorumLevel {
    pub fn parse(s: &str) -> Option<Self> {
        let u = s.to_ascii_uppercase();
        match u.as_str() {
            "ONE" => Some(Self::One),
            "LOCAL_ONE" => Some(Self::LocalOne),
            "TWO" => Some(Self::Two),
            "QUORUM" => Some(Self::Quorum),
            "ALL" => Some(Self::All),
            _ => {
                if let Some(rest) = u.strip_prefix("ACKS(").and_then(|r| r.strip_suffix(')')) {
                    rest.parse().ok().map(Self::Acks)
                } else {
                    None
                }
            }
        }
    }

    pub fn required_acks(self, live_source_replicas: u32) -> u32 {
        match self {
            Self::One | Self::LocalOne => 1,
            Self::Two => 2,
            Self::Quorum => (live_source_replicas / 2) + 1,
            Self::All => live_source_replicas,
            Self::Acks(n) => n,
        }
    }
}

/// Product defaults: write TWO, read ONE. One-node starter clamps write to ONE.
pub fn product_defaults(source_replica_count: u32) -> (QuorumLevel, QuorumLevel) {
    let write = if source_replica_count <= 1 {
        QuorumLevel::One
    } else {
        QuorumLevel::Two
    };
    (write, QuorumLevel::One)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_arithmetic() {
        assert_eq!(
            product_defaults(1),
            (QuorumLevel::One, QuorumLevel::One)
        );
        assert_eq!(
            product_defaults(3),
            (QuorumLevel::Two, QuorumLevel::One)
        );
        assert_eq!(QuorumLevel::Quorum.required_acks(3), 2);
        assert_eq!(QuorumLevel::Two.required_acks(3), 2);
    }
}

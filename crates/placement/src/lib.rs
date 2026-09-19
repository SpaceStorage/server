//! Topology ladder + quorum helpers (004 MVP).

#[derive(Debug, Clone)]
pub struct Topology {
    pub ladder: Vec<String>,
    pub quorum_domain: String,
}

impl Topology {
    pub fn default_lab() -> Self {
        Self {
            ladder: vec!["az".into()],
            quorum_domain: "lab".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quorum {
    One,
    Two,
}

impl Quorum {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "ONE" | "1" => Some(Self::One),
            "TWO" | "2" => Some(Self::Two),
            _ => None,
        }
    }

    pub fn required_acks(self) -> u32 {
        match self {
            Self::One => 1,
            Self::Two => 2,
        }
    }
}

/// TWO is never reinterpreted as min(2, live_replicas).
pub fn write_satisfied(required: Quorum, durable_acks_in_domain: u32, _live_replicas: u32) -> bool {
    durable_acks_in_domain >= required.required_acks()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_not_min_live() {
        assert!(!write_satisfied(Quorum::Two, 1, 1));
        assert!(write_satisfied(Quorum::Two, 2, 2));
        assert!(!write_satisfied(Quorum::Two, 1, 3));
    }
}

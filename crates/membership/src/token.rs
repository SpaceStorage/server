//! One-time join tokens bound to node_name.

use crate::error::{MembershipError, Result};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JoinToken {
    pub token_id: Uuid,
    pub node_name: String,
    pub node_id: Option<Uuid>,
    pub expires_at_unix_ms: u64,
    pub used: bool,
}

impl JoinToken {
    pub fn mint(
        node_name: impl Into<String>,
        node_id: Option<Uuid>,
        ttl: Duration,
    ) -> Result<Self> {
        let secs = ttl.as_secs();
        if !(3600..=72 * 3600).contains(&secs) {
            return Err(MembershipError::InvalidState(
                "join_token_ttl_range".into(),
            ));
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Ok(Self {
            token_id: Uuid::new_v4(),
            node_name: node_name.into(),
            node_id,
            expires_at_unix_ms: now + ttl.as_millis() as u64,
            used: false,
        })
    }

    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        now > self.expires_at_unix_ms
    }

    pub fn consume(&mut self, node_name: &str, node_id: Uuid) -> Result<()> {
        if self.used {
            return Err(MembershipError::TokenRefused("used".into()));
        }
        if self.is_expired() {
            return Err(MembershipError::TokenRefused("expired".into()));
        }
        if self.node_name != node_name {
            return Err(MembershipError::TokenRefused("name_mismatch".into()));
        }
        if let Some(bound) = self.node_id {
            if bound != node_id {
                return Err(MembershipError::TokenRefused("id_mismatch".into()));
            }
        }
        self.used = true;
        Ok(())
    }
}

pub fn default_ttl() -> Duration {
    Duration::from_secs(12 * 3600)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_use_and_name_bind() {
        let mut t = JoinToken::mint("n2", None, default_ttl()).unwrap();
        let id = Uuid::new_v4();
        t.consume("n2", id).unwrap();
        assert!(t.consume("n2", id).is_err());
        let mut t2 = JoinToken::mint("n2", None, default_ttl()).unwrap();
        assert!(t2.consume("other", id).is_err());
    }

    #[test]
    fn ttl_range() {
        assert!(JoinToken::mint("n", None, Duration::from_secs(30)).is_err());
        assert!(JoinToken::mint("n", None, Duration::from_secs(3600)).is_ok());
    }
}

//! Principal id, login, and record shapes (014 foundational stubs).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type PrincipalId = Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LoginName(String);

impl LoginName {
    pub fn parse(s: &str) -> Result<Self, &'static str> {
        if s.is_empty() || s.len() > 63 {
            return Err("login length");
        }
        let mut chars = s.chars();
        let first = chars.next().ok_or("login empty")?;
        if !first.is_ascii_alphabetic() {
            return Err("login must start with letter");
        }
        if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err("login charset");
        }
        Ok(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScramVerifier {
    pub salt: Vec<u8>,
    pub iterations: u32,
    pub stored_key: Vec<u8>,
    pub server_key: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrincipalRecord {
    pub id: PrincipalId,
    pub login: LoginName,
    pub scram: ScramVerifier,
    pub credential_generation: u64,
    pub enabled: bool,
    pub bootstrap: bool,
}

//! Cluster identity / membership bootstrap (011 MVP).

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum Error {
    #[error("join refused: {0}")]
    JoinRefused(String),
    #[error("{0}")]
    Msg(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeIdentity {
    pub uuid: Uuid,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MembershipView {
    pub members: Vec<NodeIdentity>,
    pub bootstrap: bool,
}

impl MembershipView {
    pub fn bootstrap(name: impl Into<String>) -> Self {
        Self {
            members: vec![NodeIdentity {
                uuid: Uuid::new_v4(),
                name: name.into(),
            }],
            bootstrap: true,
        }
    }

    pub fn join(&mut self, name: impl Into<String>, secret_ok: bool) -> Result<NodeIdentity, Error> {
        if !secret_ok {
            return Err(Error::JoinRefused("bad join secret".into()));
        }
        let id = NodeIdentity {
            uuid: Uuid::new_v4(),
            name: name.into(),
        };
        self.members.push(id.clone());
        Ok(id)
    }
}

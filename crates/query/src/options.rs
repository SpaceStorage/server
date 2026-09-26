//! Query options — isolation, concurrency, partial, async (005).

use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum IsolationLevel {
    #[default]
    ReadCommitted,
    Snapshot,
}

impl IsolationLevel {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_uppercase().as_str() {
            "READ COMMITTED" | "READ_COMMITTED" | "RC" => Ok(Self::ReadCommitted),
            "SNAPSHOT" | "REPEATABLE READ" | "REPEATABLE_READ" => Ok(Self::Snapshot),
            "SERIALIZABLE" => Err("serializable_refused".into()),
            other => Err(format!("unknown_isolation:{other}")),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadCommitted => "READ COMMITTED",
            Self::Snapshot => "SNAPSHOT",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Concurrency {
    Sequential,
    Parallel { degree: u16 },
}

impl Default for Concurrency {
    fn default() -> Self {
        Self::Sequential
    }
}

impl Concurrency {
    pub fn degree(self) -> u16 {
        match self {
            Self::Sequential => 1,
            Self::Parallel { degree } => degree.max(1),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sourced<T> {
    BuiltIn(T),
    Configured(T),
    Session(T),
    Query(T),
}

impl<T> Sourced<T> {
    pub fn value(&self) -> &T {
        match self {
            Self::BuiltIn(v) | Self::Configured(v) | Self::Session(v) | Self::Query(v) => v,
        }
    }

    pub fn into_value(self) -> T {
        match self {
            Self::BuiltIn(v) | Self::Configured(v) | Self::Session(v) | Self::Query(v) => v,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryOptions {
    pub timeout: Sourced<Duration>,
    pub quorum_write: Sourced<String>,
    pub quorum_read: Sourced<String>,
    pub isolation: Sourced<IsolationLevel>,
    pub concurrency: Sourced<Concurrency>,
    pub partial_ok: Sourced<bool>,
    pub async_job: Sourced<bool>,
    /// Recorded when default quorum was clamped.
    pub quorum_clamped: bool,
}

impl Default for QueryOptions {
    fn default() -> Self {
        Self {
            timeout: Sourced::BuiltIn(Duration::from_secs(30)),
            quorum_write: Sourced::BuiltIn("TWO".into()),
            quorum_read: Sourced::BuiltIn("ONE".into()),
            isolation: Sourced::BuiltIn(IsolationLevel::ReadCommitted),
            concurrency: Sourced::BuiltIn(Concurrency::Sequential),
            partial_ok: Sourced::BuiltIn(false),
            async_job: Sourced::BuiltIn(false),
            quorum_clamped: false,
        }
    }
}

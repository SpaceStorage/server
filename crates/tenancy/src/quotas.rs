//! Quota specs and units (logical; RF MUST NOT multiply).

use crate::error::{Result, TenancyError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaUnit {
    Bytes,
    Objects,
    Connections,
    OpsPerSec,
}

impl QuotaUnit {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "bytes" => Ok(Self::Bytes),
            "objects" => Ok(Self::Objects),
            "connections" => Ok(Self::Connections),
            "ops_per_sec" => Ok(Self::OpsPerSec),
            other => Err(TenancyError::QuotaUnitUnknown {
                unit: other.into(),
            }),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bytes => "bytes",
            Self::Objects => "objects",
            Self::Connections => "connections",
            Self::OpsPerSec => "ops_per_sec",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuotaSpec {
    pub unit: QuotaUnit,
    /// `0` = reject consuming work immediately.
    pub limit: u64,
    pub datatype: Option<String>,
}

impl QuotaSpec {
    pub fn new(unit: QuotaUnit, limit: u64) -> Result<Self> {
        // Negative is impossible for u64; config parse maps signed to QuotaNegative.
        Ok(Self {
            unit,
            limit,
            datatype: None,
        })
    }
}

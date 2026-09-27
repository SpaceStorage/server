//! Later-not-first product surfaces mounted on the node (full v1 Track 2).
//!
//! Holds L0 / legal-hold / CDC / compositions / billing rates and exposes
//! request helpers used by admin-http.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use spacestorage_observability::{estimate_charge, BillingRates, ChargeEstimate, UsageSnapshot};
use spacestorage_storage::{EraseRecord, EraseRequest, LegalHold, LegalHoldStore};
use spacestorage_types::{
    CdcCatalog, CdcEvent, CompositionCatalog, CompositionKind, CompositionMember, L0Catalog,
    PlanetaryPlacement, StorageModeChoice,
};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// Shared later-surface state for a running node.
#[derive(Clone, Default)]
pub struct LaterSurfaces {
    pub l0: Arc<Mutex<L0Catalog>>,
    pub legal: Arc<Mutex<LegalHoldStore>>,
    pub cdc: Arc<Mutex<CdcCatalog>>,
    pub compositions: Arc<Mutex<CompositionCatalog>>,
    pub billing_rates: Arc<Mutex<BillingRates>>,
}

impl LaterSurfaces {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create_l0(
        &self,
        namespace: &str,
        name: &str,
        type_name: &str,
        mode: StorageModeChoice,
    ) -> Result<Uuid, String> {
        self.l0
            .lock()
            .create(namespace, name, type_name, mode)
            .map_err(|e| e.to_string())
    }

    pub fn describe_l0(&self, namespace: &str, name: &str) -> Result<serde_json::Value, String> {
        let g = self.l0.lock();
        let c = g.describe(namespace, name).map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "id": c.id,
            "namespace": c.namespace.as_str(),
            "name": c.name.as_str(),
            "type_name": c.type_name,
            "len": c.len(),
        }))
    }

    pub fn place_hold(&self, hold: LegalHold) -> Uuid {
        self.legal.lock().place_hold(hold)
    }

    pub fn release_hold(&self, id: Uuid) -> Result<(), String> {
        self.legal.lock().release_hold(id).map_err(|e| e.to_string())
    }

    pub fn erase(&self, req: EraseRequest) -> Result<EraseRecord, String> {
        self.legal.lock().erase(req).map_err(|e| e.to_string())
    }

    pub fn create_cdc(
        &self,
        namespace: &str,
        name: &str,
        source: &str,
    ) -> Result<Uuid, String> {
        self.cdc
            .lock()
            .create(namespace, name, source)
            .map_err(|e| e.to_string())
    }

    pub fn publish_cdc(
        &self,
        namespace: &str,
        name: &str,
        event: CdcEvent,
    ) -> Result<u64, String> {
        Ok(self
            .cdc
            .lock()
            .get_mut(namespace, name)
            .map_err(|e| e.to_string())?
            .publish(event))
    }

    pub fn create_composition(
        &self,
        namespace: &str,
        name: &str,
        kind: &str,
        members: Vec<CompositionMember>,
        placement: PlanetaryPlacement,
    ) -> Result<Uuid, String> {
        let k = CompositionKind::parse(kind).ok_or_else(|| format!("unknown_kind:{kind}"))?;
        self.compositions
            .lock()
            .create(namespace, name, k, members, placement)
            .map_err(|e| e.to_string())
    }

    pub fn estimate_billing(&self, usage: &UsageSnapshot) -> ChargeEstimate {
        let rates = self.billing_rates.lock().clone();
        estimate_charge(&rates, usage)
    }
}

#[derive(Debug, Deserialize)]
pub struct L0CreateBody {
    pub namespace: String,
    pub name: String,
    pub type_name: String,
    #[serde(default)]
    pub mode: Option<String>,
}

impl L0CreateBody {
    pub fn mode(&self) -> StorageModeChoice {
        match self.mode.as_deref() {
            Some("persistent") => StorageModeChoice::Persistent,
            Some("hybrid") => StorageModeChoice::Hybrid,
            _ => StorageModeChoice::Memory,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct HoldBody {
    pub namespace: String,
    pub container: String,
    #[serde(default)]
    pub key: Option<String>,
    pub reason: String,
}

#[derive(Debug, Deserialize)]
pub struct EraseBody {
    pub namespace: String,
    pub container: String,
    pub key: String,
}

#[derive(Debug, Deserialize)]
pub struct CdcCreateBody {
    pub namespace: String,
    pub name: String,
    pub source_container: String,
}

#[derive(Debug, Deserialize)]
pub struct CompositionCreateBody {
    pub namespace: String,
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub members: Vec<CompositionMemberBody>,
    #[serde(default)]
    pub placement: PlacementBody,
}

#[derive(Debug, Deserialize)]
pub struct CompositionMemberBody {
    #[serde(default)]
    pub namespace: Option<String>,
    pub container: String,
    #[serde(default)]
    pub route_key: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct PlacementBody {
    #[serde(default)]
    pub labels: HashMap<String, String>,
    #[serde(default)]
    pub quorum_domain: String,
}

impl CompositionCreateBody {
    pub fn into_parts(self) -> (Vec<CompositionMember>, PlanetaryPlacement) {
        let ns_default = self.namespace.clone();
        let members = self
            .members
            .into_iter()
            .map(|m| CompositionMember {
                namespace: m.namespace.unwrap_or_else(|| ns_default.clone()),
                container: m.container,
                route_key: m.route_key,
            })
            .collect();
        let placement = PlanetaryPlacement {
            labels: self.placement.labels,
            quorum_domain: if self.placement.quorum_domain.is_empty() {
                "default".into()
            } else {
                self.placement.quorum_domain
            },
        };
        (members, placement)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BillingEstimateBody {
    pub namespace: String,
    #[serde(default)]
    pub datatype: String,
    pub usage_bytes: u64,
    pub usage_objects: u64,
    #[serde(default)]
    pub connections: u64,
    #[serde(default = "one")]
    pub month_fraction: u64,
    #[serde(default)]
    pub connection_hours: u64,
}

fn one() -> u64 {
    1
}

impl BillingEstimateBody {
    pub fn usage(&self) -> UsageSnapshot {
        UsageSnapshot {
            namespace: self.namespace.clone(),
            datatype: if self.datatype.is_empty() {
                "kv_store".into()
            } else {
                self.datatype.clone()
            },
            usage_bytes: self.usage_bytes,
            usage_objects: self.usage_objects,
            connections: self.connections,
            month_fraction: self.month_fraction,
            connection_hours: self.connection_hours,
        }
    }
}

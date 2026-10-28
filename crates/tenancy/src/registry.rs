//! Namespace registry (cluster-store applied view).

use crate::error::{Result, TenancyError};
use crate::name::validate_namespace_name;
use crate::quotas::QuotaSpec;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

pub type NamespaceId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NamespaceName(String);

impl NamespaceName {
    pub fn parse(s: &str) -> Result<Self> {
        validate_namespace_name(s)?;
        Ok(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NamespaceRecord {
    pub id: NamespaceId,
    pub name: String,
    pub quotas: Vec<QuotaSpec>,
    pub policies: Vec<serde_json::Value>,
    pub private_metrics: bool,
    pub private_logs: bool,
    pub group_id: String,
    pub deleted: bool,
}

impl NamespaceRecord {
    pub fn new(name: impl Into<String>) -> Result<Self> {
        let name = name.into();
        validate_namespace_name(&name)?;
        Self::with_id(Uuid::new_v4(), name)
    }

    /// Deterministic cluster-log apply constructor: the leader assigns `id` in the log body.
    pub fn with_id(id: Uuid, name: impl Into<String>) -> Result<Self> {
        let name = name.into();
        validate_namespace_name(&name)?;
        Ok(Self {
            id,
            name,
            quotas: Vec::new(),
            policies: Vec::new(),
            private_metrics: false,
            private_logs: false,
            group_id: format!("ns/{id}"),
            deleted: false,
        })
    }

    /// `on` waits for observability export (`08`). Omitted / `off` is allowed.
    pub fn set_private_telemetry(
        &mut self,
        private_metrics: bool,
        private_logs: bool,
    ) -> Result<()> {
        if private_metrics {
            return Err(TenancyError::ObservabilityRequired {
                op: "private_metrics".into(),
            });
        }
        if private_logs {
            return Err(TenancyError::ObservabilityRequired {
                op: "private_logs".into(),
            });
        }
        self.private_metrics = false;
        self.private_logs = false;
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct NamespaceRegistry {
    by_id: BTreeMap<Uuid, NamespaceRecord>,
    by_name: BTreeMap<String, Uuid>,
}

impl NamespaceRegistry {
    pub fn create(&mut self, verb_bits: u16, name: &str) -> Result<NamespaceRecord> {
        crate::ops::require_cluster_admin(verb_bits)?;
        validate_namespace_name(name)?;
        if self.by_name.contains_key(name) {
            return Err(TenancyError::NamespaceExists { name: name.into() });
        }
        let rec = NamespaceRecord::new(name)?;
        self.by_name.insert(rec.name.clone(), rec.id);
        self.by_id.insert(rec.id, rec.clone());
        Ok(rec)
    }

    pub fn rename(&mut self, verb_bits: u16, id: Uuid, new_name: &str) -> Result<()> {
        crate::ops::require_cluster_admin(verb_bits)?;
        validate_namespace_name(new_name)?;
        if self.by_name.contains_key(new_name) {
            return Err(TenancyError::NamespaceExists {
                name: new_name.into(),
            });
        }
        let rec = self
            .by_id
            .get_mut(&id)
            .ok_or_else(|| TenancyError::NamespaceNotFound {
                name: id.to_string(),
            })?;
        let old = rec.name.clone();
        rec.name = new_name.into();
        self.by_name.remove(&old);
        self.by_name.insert(new_name.into(), id);
        Ok(())
    }

    pub fn delete(
        &mut self,
        verb_bits: u16,
        id: Uuid,
        cascade: bool,
        containers: &[Uuid],
    ) -> Result<()> {
        crate::ops::require_cluster_admin(verb_bits)?;
        let rec = self
            .by_id
            .get(&id)
            .ok_or_else(|| TenancyError::NamespaceNotFound {
                name: id.to_string(),
            })?;
        if !cascade && !containers.is_empty() {
            return Err(TenancyError::CascadeRequired {
                name: rec.name.clone(),
                containers: containers.to_vec(),
            });
        }
        let name = rec.name.clone();
        self.by_name.remove(&name);
        self.by_id.remove(&id);
        Ok(())
    }

    pub fn get_by_name(&self, name: &str) -> Option<&NamespaceRecord> {
        self.by_name.get(name).and_then(|id| self.by_id.get(id))
    }

    pub fn get_by_id(&self, id: uuid::Uuid) -> Option<&NamespaceRecord> {
        self.by_id.get(&id)
    }

    pub fn list(&self) -> Vec<&NamespaceRecord> {
        self.by_id.values().collect()
    }

    /// Replace the local read cache from cluster-applied records.
    pub fn replace_applied(&mut self, records: impl IntoIterator<Item = NamespaceRecord>) {
        self.by_id.clear();
        self.by_name.clear();
        for record in records {
            self.by_name.insert(record.name.clone(), record.id);
            self.by_id.insert(record.id, record);
        }
    }

    pub fn replace_quotas(
        &mut self,
        verb_bits: u16,
        id: Uuid,
        quotas: Vec<QuotaSpec>,
    ) -> Result<()> {
        crate::ops::require_cluster_admin(verb_bits)?;
        #[cfg(not(feature = "tenancy-quotas"))]
        {
            let _ = (id, quotas);
            return Err(TenancyError::Slice7Required {
                op: "QuotaReplace".into(),
            });
        }
        #[cfg(feature = "tenancy-quotas")]
        {
            let rec = self
                .by_id
                .get_mut(&id)
                .ok_or_else(|| TenancyError::NamespaceNotFound {
                    name: id.to_string(),
                })?;
            rec.quotas = quotas;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spacestorage_authz::Verb;

    fn admin() -> u16 {
        Verb::CLUSTER_ADMIN.bits()
    }

    #[test]
    fn create_rename_unique() {
        let mut r = NamespaceRegistry::default();
        let a = r.create(admin(), "acme").unwrap();
        assert!(r.create(admin(), "acme").is_err());
        r.rename(admin(), a.id, "contoso").unwrap();
        assert!(r.get_by_name("acme").is_none());
        assert!(r.get_by_name("contoso").is_some());
        assert_eq!(r.get_by_name("contoso").unwrap().id, a.id);
    }

    #[test]
    fn cascade_required() {
        let mut r = NamespaceRegistry::default();
        let a = r.create(admin(), "acme").unwrap();
        let c = Uuid::from_u128(1);
        let err = r.delete(admin(), a.id, false, &[c]).unwrap_err();
        assert!(matches!(err, TenancyError::CascadeRequired { .. }));
    }

    #[test]
    fn namespace_admin_cannot_create_or_replace_quotas() {
        let mut r = NamespaceRegistry::default();
        let bits = Verb::NAMESPACE_ADMIN.bits();
        assert!(matches!(
            r.create(bits, "acme"),
            Err(TenancyError::NotClusterAdmin)
        ));
        let id = Uuid::from_u128(1);
        assert!(matches!(
            r.replace_quotas(bits, id, Vec::new()),
            Err(TenancyError::NotClusterAdmin)
        ));
        assert!(matches!(
            r.replace_quotas(Verb::CONFIGURE.bits(), id, Vec::new()),
            Err(TenancyError::NotClusterAdmin)
        ));
    }

    #[test]
    fn private_telemetry_on_requires_observability() {
        let mut rec = NamespaceRecord::new("acme").unwrap();
        assert!(rec.set_private_telemetry(false, false).is_ok());
        assert!(matches!(
            rec.set_private_telemetry(true, false),
            Err(TenancyError::ObservabilityRequired { .. })
        ));
        assert!(matches!(
            rec.set_private_telemetry(false, true),
            Err(TenancyError::ObservabilityRequired { .. })
        ));
        assert!(!rec.private_metrics && !rec.private_logs);
    }
}

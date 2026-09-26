//! Release profile enumeration.

use crate::handlers::HandlerBuildSet;
use crate::types::TypeRequirement;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReleaseProfile {
    FirstBinary,
    /// Slice-6 gate: all eight protocol handlers at `015` HandlersComplete MUST.
    HandlersComplete,
    /// Slice-7 gate: Raft control plane, tenancy quotas, full authz vocabulary.
    ControlPlaneTenancyAuthz,
    /// Slice-8 gate: query beyond CRUD (`query-distributed`).
    QueryDistributed,
    /// Slice-9 gate: full `008` metrics catalog on `/metrics`.
    ObservabilityCatalog,
    /// Slice-10 gate: migration/transforms + backup/PITR.
    MigrationBackup,
    CompleteProduct,
}

impl ReleaseProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FirstBinary => "first-binary",
            Self::HandlersComplete => "handlers-complete",
            Self::ControlPlaneTenancyAuthz => "control-plane-tenancy-authz",
            Self::QueryDistributed => "query-distributed",
            Self::ObservabilityCatalog => "observability-catalog",
            Self::MigrationBackup => "migration-backup",
            Self::CompleteProduct => "complete-product",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "first-binary" => Some(Self::FirstBinary),
            "handlers-complete" => Some(Self::HandlersComplete),
            "control-plane-tenancy-authz" => Some(Self::ControlPlaneTenancyAuthz),
            "query-distributed" | "query-beyond-crud" => Some(Self::QueryDistributed),
            "observability-catalog" | "observability" => Some(Self::ObservabilityCatalog),
            "migration-backup" | "migrate" => Some(Self::MigrationBackup),
            "complete-product" => Some(Self::CompleteProduct),
            _ => None,
        }
    }

    pub fn handler_set(self) -> HandlerBuildSet {
        HandlerBuildSet::for_profile(self)
    }

    pub fn type_requirement(self) -> TypeRequirement {
        TypeRequirement::for_profile(self)
    }

    /// Expected implemented max `k` for this profile.
    pub fn expected_implemented_max(self) -> u8 {
        match self {
            Self::FirstBinary => 5,
            Self::HandlersComplete => 6,
            Self::ControlPlaneTenancyAuthz => 7,
            Self::QueryDistributed => 8,
            Self::ObservabilityCatalog => 9,
            Self::MigrationBackup => 10,
            Self::CompleteProduct => 11,
        }
    }

    /// Map onto the 015 dialect profile name (`spacestorage-compat::DialectProfile`).
    pub fn dialect_profile_name(self) -> &'static str {
        match self {
            Self::FirstBinary => "first-binary",
            Self::HandlersComplete => "handlers-complete",
            // Slice 7 does not expand dialect; keep HC dialect name for compat mapping.
            Self::ControlPlaneTenancyAuthz => "handlers-complete",
            Self::QueryDistributed
            | Self::ObservabilityCatalog
            | Self::MigrationBackup
            | Self::CompleteProduct => "complete-product",
        }
    }
}

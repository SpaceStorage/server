//! Slice identifiers for the implementation ledger (slices 1..=11).

use crate::error::ValidationCode;

/// Implementation slice id. Stable forever; specify order stays `01`–`16`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum SliceId {
    Runtime = 1,
    TypesDurability = 2,
    PostgreSqlSubset = 3,
    RedisSubset = 4,
    MembershipInternodeQuorum = 5,
    RemainingHandlers = 6,
    ControlPlaneTenancyAuthz = 7,
    QueryBeyondCrud = 8,
    ObservabilityCatalog = 9,
    MigrationBackup = 10,
    UisIngest = 11,
}

/// First-binary slices: `1..=5`.
pub const FIRST_BINARY: [SliceId; 5] = [
    SliceId::Runtime,
    SliceId::TypesDurability,
    SliceId::PostgreSqlSubset,
    SliceId::RedisSubset,
    SliceId::MembershipInternodeQuorum,
];

/// Deferred after first binary: `{6,7,8,9,10,11}`.
pub const DEFERRED_AFTER_FIRST: [SliceId; 6] = [
    SliceId::RemainingHandlers,
    SliceId::ControlPlaneTenancyAuthz,
    SliceId::QueryBeyondCrud,
    SliceId::ObservabilityCatalog,
    SliceId::MigrationBackup,
    SliceId::UisIngest,
];

impl SliceId {
    pub const MIN: u8 = 1;
    pub const MAX: u8 = 11;

    pub fn from_u8(id: u8) -> Result<Self, ValidationCode> {
        match id {
            1 => Ok(Self::Runtime),
            2 => Ok(Self::TypesDurability),
            3 => Ok(Self::PostgreSqlSubset),
            4 => Ok(Self::RedisSubset),
            5 => Ok(Self::MembershipInternodeQuorum),
            6 => Ok(Self::RemainingHandlers),
            7 => Ok(Self::ControlPlaneTenancyAuthz),
            8 => Ok(Self::QueryBeyondCrud),
            9 => Ok(Self::ObservabilityCatalog),
            10 => Ok(Self::MigrationBackup),
            11 => Ok(Self::UisIngest),
            _ => Err(ValidationCode::SliceUnknown { id }),
        }
    }

    pub fn id(self) -> u8 {
        self as u8
    }

    /// `true` iff this slice is part of the first shippable binary (`id <= 5`).
    pub fn first_binary(self) -> bool {
        self.id() <= 5
    }

    /// Intent / spec files this slice implements (not specifies).
    /// Paths are relative to the repository root under `specs/` or `.specify/`.
    pub fn intent_files(self) -> &'static [&'static str] {
        match self {
            Self::Runtime => &["specs/001-runtime-cli-api"],
            Self::TypesDurability => &[
                "specs/003-type-system",
                "specs/013-durability-and-recovery",
                "specs/014-authz-keys", // master-key subset
            ],
            Self::PostgreSqlSubset => &["specs/002-protocol-drivers"], // postgresql
            Self::RedisSubset => &[
                "specs/002-protocol-drivers", // redis
                "specs/015-compatibility-and-limits", // KV MUST
            ],
            Self::MembershipInternodeQuorum => &[
                "specs/011-identity-membership",
                "specs/012-internode-and-time",
                "specs/004-distribution-placement", // quorum
            ],
            Self::RemainingHandlers => &[
                "specs/002-protocol-drivers",
                "specs/015-compatibility-and-limits",
            ],
            Self::ControlPlaneTenancyAuthz => &[
                "specs/006-control-plane",
                "specs/007-tenancy-security",
                "specs/014-authz-keys",
            ],
            Self::QueryBeyondCrud => &["specs/005-query-execution"],
            Self::ObservabilityCatalog => &["specs/008-observability"],
            Self::MigrationBackup => &[
                "specs/010-migration-transforms",
                "specs/013-durability-and-recovery",
            ],
            Self::UisIngest => &["specs/009-admin-ui-ingest"],
        }
    }

    pub fn all() -> [Self; 11] {
        [
            Self::Runtime,
            Self::TypesDurability,
            Self::PostgreSqlSubset,
            Self::RedisSubset,
            Self::MembershipInternodeQuorum,
            Self::RemainingHandlers,
            Self::ControlPlaneTenancyAuthz,
            Self::QueryBeyondCrud,
            Self::ObservabilityCatalog,
            Self::MigrationBackup,
            Self::UisIngest,
        ]
    }
}

/// Validate that `implemented` is a contiguous prefix `1..=k` with no holes.
pub fn validate_implemented_prefix(implemented: &[u8]) -> Result<u8, ValidationCode> {
    if implemented.is_empty() {
        return Err(ValidationCode::ImplementedNotPrefix);
    }
    let mut sorted = implemented.to_vec();
    sorted.sort_unstable();
    sorted.dedup();

    for id in &sorted {
        SliceId::from_u8(*id)?;
    }

    let k = *sorted.last().unwrap();
    let expected: Vec<u8> = (1..=k).collect();
    if sorted != expected {
        let missing: Vec<u8> = expected
            .into_iter()
            .filter(|i| !sorted.contains(i))
            .collect();
        return Err(ValidationCode::SliceGap { missing });
    }
    Ok(k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slice_gap_on_holes() {
        let err = validate_implemented_prefix(&[1, 2, 4]).unwrap_err();
        assert_eq!(err.code(), "slice_gap");
        match err {
            ValidationCode::SliceGap { missing } => assert_eq!(missing, vec![3]),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn prefix_ok() {
        assert_eq!(validate_implemented_prefix(&[1, 2, 3, 4, 5]).unwrap(), 5);
    }

    #[test]
    fn first_binary_constants() {
        assert!(FIRST_BINARY.iter().all(|s| s.first_binary()));
        assert!(DEFERRED_AFTER_FIRST.iter().all(|s| !s.first_binary()));
        assert!(!SliceId::intent_files(SliceId::Runtime).is_empty());
    }

    #[test]
    fn unknown_slice() {
        assert_eq!(
            SliceId::from_u8(0).unwrap_err().code(),
            "slice_unknown"
        );
    }
}

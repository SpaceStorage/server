//! Product non-goals (not deferred slices; not later).

use crate::error::ValidationCode;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProductNonGoal {
    SecondQueryEnginePerProtocol,
    DropInReplacementOfEmulatedSystems,
    KafkaAsStoredLogProduct,
    HumanPickedMultiMasterConflict,
    ByzantineNodes,
    PerTenantCpuHardIsolation,
    NativeClientProtocol,
    SqlSerializable,
    MultiActiveOnInFirstBinary,
}

impl ProductNonGoal {
    pub fn id(self) -> &'static str {
        match self {
            Self::SecondQueryEnginePerProtocol => "SecondQueryEnginePerProtocol",
            Self::DropInReplacementOfEmulatedSystems => "DropInReplacementOfEmulatedSystems",
            Self::KafkaAsStoredLogProduct => "KafkaAsStoredLogProduct",
            Self::HumanPickedMultiMasterConflict => "HumanPickedMultiMasterConflict",
            Self::ByzantineNodes => "ByzantineNodes",
            Self::PerTenantCpuHardIsolation => "PerTenantCpuHardIsolation",
            Self::NativeClientProtocol => "NativeClientProtocol",
            Self::SqlSerializable => "SqlSerializable",
            Self::MultiActiveOnInFirstBinary => "MultiActiveOnInFirstBinary",
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::SecondQueryEnginePerProtocol,
            Self::DropInReplacementOfEmulatedSystems,
            Self::KafkaAsStoredLogProduct,
            Self::HumanPickedMultiMasterConflict,
            Self::ByzantineNodes,
            Self::PerTenantCpuHardIsolation,
            Self::NativeClientProtocol,
            Self::SqlSerializable,
            Self::MultiActiveOnInFirstBinary,
        ]
    }
}

/// Paths (repo-relative) that MUST mention each non-goal (or its statement).
pub const REQUIRED_MENTIONS: &[(ProductNonGoal, &[&str])] = &[
    (
        ProductNonGoal::SecondQueryEnginePerProtocol,
        &[
            "specs/002-protocol-drivers/spec.md",
            "specs/005-query-execution/spec.md",
        ],
    ),
    (
        ProductNonGoal::DropInReplacementOfEmulatedSystems,
        &["specs/015-compatibility-and-limits/spec.md"],
    ),
    (
        ProductNonGoal::KafkaAsStoredLogProduct,
        &[
            "specs/003-type-system/spec.md",
            "specs/008-observability/spec.md",
            "specs/009-admin-ui-ingest/spec.md",
        ],
    ),
    (
        ProductNonGoal::HumanPickedMultiMasterConflict,
        &["specs/012-internode-and-time/spec.md"],
    ),
    (
        ProductNonGoal::ByzantineNodes,
        &["specs/012-internode-and-time/spec.md"],
    ),
    (
        ProductNonGoal::PerTenantCpuHardIsolation,
        &[
            "specs/015-compatibility-and-limits/spec.md",
            "specs/007-tenancy-security/spec.md",
        ],
    ),
    (
        ProductNonGoal::NativeClientProtocol,
        &[
            ".specify/memory/constitution.md",
            "specs/002-protocol-drivers/spec.md",
        ],
    ),
    (
        ProductNonGoal::SqlSerializable,
        &[
            "specs/015-compatibility-and-limits/spec.md",
            "specs/005-query-execution/spec.md",
        ],
    ),
    (
        ProductNonGoal::MultiActiveOnInFirstBinary,
        &[
            "specs/012-internode-and-time/spec.md",
            "specs/016-mvp-and-nongoals/spec.md",
        ],
    ),
];

/// Needle phrases that count as a mention for each non-goal.
fn needles(goal: ProductNonGoal) -> &'static [&'static str] {
    match goal {
        ProductNonGoal::SecondQueryEnginePerProtocol => &[
            "second query engine",
            "SecondQueryEnginePerProtocol",
        ],
        ProductNonGoal::DropInReplacementOfEmulatedSystems => &[
            "drop-in replacement",
            "DropInReplacementOfEmulatedSystems",
            "unmodified **applications**",
            "unmodified applications",
        ],
        ProductNonGoal::KafkaAsStoredLogProduct => &[
            "Kafka as a stored log",
            "not a stored log product",
            "KafkaAsStoredLogProduct",
        ],
        ProductNonGoal::HumanPickedMultiMasterConflict => &[
            "wait for a human",
            "HumanPickedMultiMasterConflict",
            "pause-for-human",
        ],
        ProductNonGoal::ByzantineNodes => &["not Byzantine", "ByzantineNodes", "Byzantine"],
        ProductNonGoal::PerTenantCpuHardIsolation => &[
            "CPU hard isolation",
            "PerTenantCpuHardIsolation",
        ],
        ProductNonGoal::NativeClientProtocol => &[
            "SpaceStorage-native",
            "NativeClientProtocol",
            "handler spacestorage",
        ],
        ProductNonGoal::SqlSerializable => &["SERIALIZABLE", "SqlSerializable"],
        ProductNonGoal::MultiActiveOnInFirstBinary => &[
            "multi_active",
            "MultiActiveOnInFirstBinary",
            "multi_active_unsupported",
        ],
    }
}

pub fn audit_nongoals(repo_root: &Path) -> Result<(), ValidationCode> {
    for (goal, paths) in REQUIRED_MENTIONS {
        for rel in *paths {
            let path = repo_root.join(rel);
            let text = fs::read_to_string(&path).map_err(|_| ValidationCode::NongoalUnspecified {
                id: goal.id().to_string(),
                spec: rel.to_string(),
            })?;
            let lower = text.to_lowercase();
            let found = needles(*goal).iter().any(|n| {
                if n.chars().any(|c| c.is_ascii_uppercase()) && *n != &n.to_lowercase() {
                    // mixed / exact-ish: try both
                    text.contains(n) || lower.contains(&n.to_lowercase())
                } else {
                    lower.contains(&n.to_lowercase())
                }
            });
            if !found {
                return Err(ValidationCode::NongoalUnspecified {
                    id: goal.id().to_string(),
                    spec: rel.to_string(),
                });
            }
        }
    }
    Ok(())
}

/// Audit a single file contents (for negative fixtures).
pub fn audit_text_mentions_serializable(text: &str) -> bool {
    needles(ProductNonGoal::SqlSerializable)
        .iter()
        .any(|n| text.to_lowercase().contains(&n.to_lowercase()))
}

pub fn default_repo_root() -> PathBuf {
    // tests run from crate dir or workspace; walk up looking for specs/
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for _ in 0..6 {
        if dir.join("specs/016-mvp-and-nongoals").is_dir() {
            return dir;
        }
        if !dir.pop() {
            break;
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

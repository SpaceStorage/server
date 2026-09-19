//! Milestone ledger: implemented prefix + deferred slices.

use crate::error::ValidationCode;
use crate::profile::ReleaseProfile;
use crate::slice::{self, SliceId, DEFERRED_AFTER_FIRST};
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DeferredSlice {
    pub id: u8,
    pub reason: String,
    pub still_owed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MilestoneRecord {
    pub slug: String,
    pub profile: String,
    pub implemented: Vec<u8>,
    #[serde(default)]
    pub deferred: Vec<DeferredSlice>,
    pub changelog_ref: String,
}

impl MilestoneRecord {
    pub fn parse_yaml(text: &str) -> Result<Self, String> {
        serde_yaml::from_str(text).map_err(|e| e.to_string())
    }

    pub fn load_file(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
        Self::parse_yaml(&text)
    }

    pub fn release_profile(&self) -> Result<ReleaseProfile, ValidationCode> {
        ReleaseProfile::parse(&self.profile).ok_or(ValidationCode::ProfileMismatch {
            profile: self.profile.clone(),
            expected_k: 0,
            actual_k: 0,
        })
    }

    pub fn validate(&self) -> Result<(), ValidationCode> {
        let k = slice::validate_implemented_prefix(&self.implemented)?;
        let profile = self.release_profile()?;

        match profile {
            ReleaseProfile::FirstBinary => {
                if k != 5 {
                    return Err(ValidationCode::ProfileMismatch {
                        profile: profile.as_str().to_string(),
                        expected_k: 5,
                        actual_k: k,
                    });
                }
                validate_deferred_first_binary(&self.deferred)?;
            }
            ReleaseProfile::CompleteProduct => {
                if k != 11 {
                    let missing: Vec<u8> = (1..=11)
                        .filter(|i| !self.implemented.contains(i))
                        .collect();
                    return Err(ValidationCode::ProfileIncomplete {
                        profile: profile.as_str().to_string(),
                        missing,
                    });
                }
                if !self.deferred.is_empty() {
                    return Err(ValidationCode::ProfileIncomplete {
                        profile: profile.as_str().to_string(),
                        missing: vec![],
                    });
                }
            }
        }

        for d in &self.deferred {
            if d.reason.trim().is_empty() {
                return Err(ValidationCode::DeferredMissing {
                    missing: vec![d.id],
                });
            }
            // still_owed MUST be true unless the slice’s entire content is a ProductNonGoal
            // (none of 6–11 are)
            if !d.still_owed {
                return Err(ValidationCode::DeferredMarkedCancelled { id: d.id });
            }
            SliceId::from_u8(d.id)?;
        }

        Ok(())
    }
}

fn validate_deferred_first_binary(deferred: &[DeferredSlice]) -> Result<(), ValidationCode> {
    let expected: Vec<u8> = DEFERRED_AFTER_FIRST.iter().map(|s| s.id()).collect();
    let mut got: Vec<u8> = deferred.iter().map(|d| d.id).collect();
    got.sort_unstable();
    if got != expected {
        let missing: Vec<u8> = expected
            .into_iter()
            .filter(|i| !deferred.iter().any(|d| d.id == *i))
            .collect();
        return Err(ValidationCode::DeferredMissing { missing });
    }
    Ok(())
}

/// Validate all `*.yaml` milestone records under `docs/milestones/` (skip `fixtures/`).
pub fn validate_milestones_dir(dir: &Path) -> Result<(), ValidationCode> {
    let entries = fs::read_dir(dir).map_err(|_| ValidationCode::DeferredMissing {
        missing: vec![],
    })?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        let record = MilestoneRecord::load_file(&path).map_err(|_| ValidationCode::SliceUnknown {
            id: 0,
        })?;
        record.validate()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first_binary_yaml() -> &'static str {
        r#"
slug: first-binary
profile: first-binary
implemented: [1, 2, 3, 4, 5]
deferred:
  - { id: 6, still_owed: true, reason: "remaining 015 handlers" }
  - { id: 7, still_owed: true, reason: "Raft, quotas, full authz" }
  - { id: 8, still_owed: true, reason: "query beyond CRUD" }
  - { id: 9, still_owed: true, reason: "full 08 catalog" }
  - { id: 10, still_owed: true, reason: "migration and PITR" }
  - { id: 11, still_owed: true, reason: "UIs and ingest" }
changelog_ref: docs/milestones/001-first-binary.md
"#
    }

    #[test]
    fn deferred_missing_when_k5_incomplete() {
        let yaml = r#"
slug: bad
profile: first-binary
implemented: [1, 2, 3, 4, 5]
deferred:
  - { id: 6, still_owed: true, reason: "x" }
  - { id: 7, still_owed: true, reason: "x" }
  - { id: 9, still_owed: true, reason: "x" }
  - { id: 10, still_owed: true, reason: "x" }
  - { id: 11, still_owed: true, reason: "x" }
changelog_ref: x.md
"#;
        let r = MilestoneRecord::parse_yaml(yaml).unwrap();
        let err = r.validate().unwrap_err();
        assert_eq!(err.code(), "deferred_missing");
    }

    #[test]
    fn profile_mismatch_when_first_binary_not_k5() {
        let yaml = r#"
slug: bad
profile: first-binary
implemented: [1, 2, 3]
deferred:
  - { id: 4, still_owed: true, reason: "x" }
  - { id: 5, still_owed: true, reason: "x" }
  - { id: 6, still_owed: true, reason: "x" }
  - { id: 7, still_owed: true, reason: "x" }
  - { id: 8, still_owed: true, reason: "x" }
  - { id: 9, still_owed: true, reason: "x" }
  - { id: 10, still_owed: true, reason: "x" }
  - { id: 11, still_owed: true, reason: "x" }
changelog_ref: x.md
"#;
        let r = MilestoneRecord::parse_yaml(yaml).unwrap();
        let err = r.validate().unwrap_err();
        assert_eq!(err.code(), "profile_mismatch");
    }

    #[test]
    fn profile_incomplete_complete_product() {
        let yaml = r#"
slug: bad
profile: complete-product
implemented: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
deferred: []
changelog_ref: x.md
"#;
        let r = MilestoneRecord::parse_yaml(yaml).unwrap();
        let err = r.validate().unwrap_err();
        assert_eq!(err.code(), "profile_incomplete");
    }

    #[test]
    fn first_binary_ok() {
        let r = MilestoneRecord::parse_yaml(first_binary_yaml()).unwrap();
        r.validate().unwrap();
    }

    #[test]
    fn cancelled_deferred_rejected() {
        let yaml = r#"
slug: bad
profile: first-binary
implemented: [1, 2, 3, 4, 5]
deferred:
  - { id: 6, still_owed: true, reason: "x" }
  - { id: 7, still_owed: true, reason: "x" }
  - { id: 8, still_owed: false, reason: "cancelled" }
  - { id: 9, still_owed: true, reason: "x" }
  - { id: 10, still_owed: true, reason: "x" }
  - { id: 11, still_owed: true, reason: "x" }
changelog_ref: x.md
"#;
        let r = MilestoneRecord::parse_yaml(yaml).unwrap();
        assert_eq!(r.validate().unwrap_err().code(), "deferred_marked_cancelled");
    }
}

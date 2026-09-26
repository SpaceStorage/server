//! Closed isolation set: READ COMMITTED / SNAPSHOT; SERIALIZABLE refused.
//!
//! FirstBinary / HandlersComplete: `BEGIN` is MUST NOT (`begin_not_in_profile`),
//! so SNAPSHOT is not required on those profiles.

use crate::error::CompatError;
use serde::{Deserialize, Serialize};

/// Stored isolation level attached by exec on SQL txn begin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IsolationLevel {
    ReadCommitted,
    Snapshot,
}

/// Refuse codes from [`map_sql_isolation`].
pub type IsolationRefuse = CompatError;

/// Map client SQL isolation text → stored level.
///
/// | Input | Result |
/// |-------|--------|
/// | omitted / empty / `read committed` | ReadCommitted |
/// | `repeatable read` / `snapshot` | Snapshot |
/// | `read uncommitted` | ReadCommitted (documented upgrade) |
/// | `serializable` | refuse `serializable_nongoal` |
pub fn map_sql_isolation(text: &str) -> Result<IsolationLevel, IsolationRefuse> {
    let t = text.trim().to_ascii_lowercase();
    let norm = t.replace('_', " ").replace('-', " ");
    let compact: String = norm.split_whitespace().collect::<Vec<_>>().join(" ");
    match compact.as_str() {
        "" | "read committed" | "default" => Ok(IsolationLevel::ReadCommitted),
        "read uncommitted" => Ok(IsolationLevel::ReadCommitted),
        "repeatable read" | "snapshot" => Ok(IsolationLevel::Snapshot),
        "serializable" => Err(CompatError::SerializableNongoal),
        _ => Err(CompatError::CompatMustNot {
            protocol: "postgresql".into(),
            verb: format!("isolation:{compact}"),
        }),
    }
}

/// Allowed isolation names for refuse messages (FR-006).
pub const ALLOWED_ISOLATION_SET: &[&str] = &["READ COMMITTED", "SNAPSHOT"];

/// Refuse SNAPSHOT when any touched type has `snapshot_capable == false`.
///
/// First-binary type defaults (owned flag lives on `003` TypeDescriptor):
/// Relational Table / Document Store → true; K/V Store → false; new types → false.
pub fn check_snapshot_capable(
    type_name: &str,
    snapshot_capable: bool,
) -> Result<(), CompatError> {
    if snapshot_capable {
        Ok(())
    } else {
        Err(CompatError::SnapshotUnsupported {
            type_name: type_name.to_string(),
        })
    }
}

/// Check every (type_name, snapshot_capable) pair; first failure wins.
pub fn check_snapshot_for_types<'a, I>(types: I) -> Result<(), CompatError>
where
    I: IntoIterator<Item = (&'a str, bool)>,
{
    for (name, capable) in types {
        check_snapshot_capable(name, capable)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_table() {
        assert_eq!(
            map_sql_isolation("").unwrap(),
            IsolationLevel::ReadCommitted
        );
        assert_eq!(
            map_sql_isolation("read committed").unwrap(),
            IsolationLevel::ReadCommitted
        );
        assert_eq!(
            map_sql_isolation("READ_COMMITTED").unwrap(),
            IsolationLevel::ReadCommitted
        );
        assert_eq!(
            map_sql_isolation("read uncommitted").unwrap(),
            IsolationLevel::ReadCommitted
        );
        assert_eq!(
            map_sql_isolation("repeatable read").unwrap(),
            IsolationLevel::Snapshot
        );
        assert_eq!(
            map_sql_isolation("snapshot").unwrap(),
            IsolationLevel::Snapshot
        );
        assert_eq!(
            map_sql_isolation("serializable").unwrap_err(),
            CompatError::SerializableNongoal
        );
    }

    #[test]
    fn snapshot_unsupported_names_type() {
        let err = check_snapshot_capable("K/V Store", false).unwrap_err();
        assert_eq!(
            err,
            CompatError::SnapshotUnsupported {
                type_name: "K/V Store".into()
            }
        );
        assert!(check_snapshot_capable("Relational Table", true).is_ok());
    }

    #[test]
    fn mixed_types_refuse_non_capable() {
        let err = check_snapshot_for_types([
            ("Relational Table", true),
            ("K/V Store", false),
        ])
        .unwrap_err();
        assert!(matches!(err, CompatError::SnapshotUnsupported { .. }));
    }
}

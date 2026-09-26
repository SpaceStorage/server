//! PITR target mapping — HLC/ingest → per-drive LSN (013 contracts/pitr.md).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PitrError {
    #[error("pitr_unmappable: drive {drive_id}")]
    Unmappable { drive_id: String },
    #[error("pitr past last durable ack on drive {drive_id}: want {want} durable {durable}")]
    PastDurable {
        drive_id: String,
        want: u64,
        durable: u64,
    },
}

/// Operator PITR target: explicit positions or a source-domain HLC/ingest stamp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PitrTarget {
    Positions {
        positions: BTreeMap<String, u64>,
    },
    Hlc {
        /// Source-domain HLC / ingest stamp (`WalRecord.stamp`).
        hlc: u64,
    },
}

/// One WAL record stamp after the snapshot cut used for HLC→LSN mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StampLsn {
    pub stamp: u64,
    pub lsn: u64,
}

/// Map `T` → `lsn* = max { lsn | stamp ≤ T }` per drive; refuse if any drive cannot map.
pub fn map_hlc_to_positions(
    hlc: u64,
    per_drive: &BTreeMap<String, Vec<StampLsn>>,
    durable: &BTreeMap<String, u64>,
) -> Result<BTreeMap<String, u64>, PitrError> {
    let mut out = BTreeMap::new();
    for (drive_id, stamps) in per_drive {
        if stamps.is_empty() {
            return Err(PitrError::Unmappable {
                drive_id: drive_id.clone(),
            });
        }
        let mut best: Option<u64> = None;
        for s in stamps {
            if s.stamp <= hlc {
                best = Some(match best {
                    Some(b) => b.max(s.lsn),
                    None => s.lsn,
                });
            }
        }
        let Some(lsn) = best else {
            return Err(PitrError::Unmappable {
                drive_id: drive_id.clone(),
            });
        };
        if let Some(&d) = durable.get(drive_id.as_str()) {
            if lsn > d {
                return Err(PitrError::PastDurable {
                    drive_id: drive_id.clone(),
                    want: lsn,
                    durable: d,
                });
            }
        }
        out.insert(drive_id.clone(), lsn);
    }
    Ok(out)
}

/// Resolve a [`PitrTarget`] into a concrete position map.
pub fn resolve_positions(
    target: &PitrTarget,
    per_drive_stamps: &BTreeMap<String, Vec<StampLsn>>,
    durable: &BTreeMap<String, u64>,
) -> Result<BTreeMap<String, u64>, PitrError> {
    match target {
        PitrTarget::Positions { positions } => {
            for (drive_id, &lsn) in positions {
                if let Some(&d) = durable.get(drive_id.as_str()) {
                    if lsn > d {
                        return Err(PitrError::PastDurable {
                            drive_id: drive_id.clone(),
                            want: lsn,
                            durable: d,
                        });
                    }
                }
            }
            Ok(positions.clone())
        }
        PitrTarget::Hlc { hlc } => map_hlc_to_positions(*hlc, per_drive_stamps, durable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hlc_maps_max_stamp_le_t() {
        let mut stamps = BTreeMap::new();
        stamps.insert(
            "d0".into(),
            vec![
                StampLsn { stamp: 10, lsn: 1 },
                StampLsn { stamp: 20, lsn: 2 },
                StampLsn { stamp: 30, lsn: 3 },
            ],
        );
        let mut durable = BTreeMap::new();
        durable.insert("d0".into(), 10);
        let pos = map_hlc_to_positions(25, &stamps, &durable).unwrap();
        assert_eq!(pos.get("d0"), Some(&2));
    }

    #[test]
    fn unmappable_when_no_stamp_le_t() {
        let mut stamps = BTreeMap::new();
        stamps.insert(
            "d1".into(),
            vec![StampLsn { stamp: 100, lsn: 5 }],
        );
        let durable = BTreeMap::new();
        let err = map_hlc_to_positions(50, &stamps, &durable).unwrap_err();
        assert!(matches!(err, PitrError::Unmappable { drive_id } if drive_id == "d1"));
    }

    #[test]
    fn refuse_past_durable() {
        let mut stamps = BTreeMap::new();
        stamps.insert("d0".into(), vec![StampLsn { stamp: 1, lsn: 9 }]);
        let mut durable = BTreeMap::new();
        durable.insert("d0".into(), 3);
        let err = map_hlc_to_positions(10, &stamps, &durable).unwrap_err();
        assert!(matches!(err, PitrError::PastDurable { .. }));
    }
}

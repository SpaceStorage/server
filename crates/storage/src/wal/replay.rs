//! WAL segment replay: skip torn tails; mid-log CRC → quarantine.

use crate::error::StorageError;
use crate::wal::record::{parse_records, parse_segment_header, WalRecord};

pub fn replay_bytes(data: &[u8], _expected_drive: &str) -> Result<(Vec<WalRecord>, u64), StorageError> {
    if data.is_empty() {
        return Ok((Vec::new(), 0));
    }
    let (_fmt, _drive, _node, start) = parse_segment_header(data)?;
    parse_records(data, start)
}

pub fn replay_segment(data: &[u8], drive_id: &str) -> Result<(Vec<WalRecord>, u64), StorageError> {
    replay_bytes(data, drive_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quarantine::quarantine_sync;
    use crate::wal::record::{
        encode_record, encode_segment_header, WalRecord, WalRecordType, FORMAT_MAJOR,
    };
    use tempfile::tempdir;
    use uuid::Uuid;

    #[test]
    fn torn_tail_skipped() {
        let mut data = encode_segment_header(FORMAT_MAJOR, "default", "n1");
        let rec = WalRecord {
            lsn: 1,
            container_id: Uuid::nil(),
            stamp: 0,
            ty: WalRecordType::Put,
            payload: b"ok".to_vec(),
        };
        data.extend_from_slice(&encode_record(&rec));
        data.extend_from_slice(&[1, 2, 3]); // torn
        let (recs, torn) = replay_bytes(&data, "default").unwrap();
        assert_eq!(recs.len(), 1);
        assert_eq!(torn, 1);
    }

    #[test]
    fn mid_log_crc_returns_corrupt_then_quarantine() {
        let mut data = encode_segment_header(FORMAT_MAJOR, "default", "n1");
        data.extend_from_slice(&encode_record(&WalRecord {
            lsn: 1,
            container_id: Uuid::nil(),
            stamp: 0,
            ty: WalRecordType::Put,
            payload: b"a".to_vec(),
        }));
        let mut bad = encode_record(&WalRecord {
            lsn: 2,
            container_id: Uuid::nil(),
            stamp: 0,
            ty: WalRecordType::Put,
            payload: b"b".to_vec(),
        });
        bad[4] ^= 0xff;
        data.extend_from_slice(&bad);
        data.extend_from_slice(&encode_record(&WalRecord {
            lsn: 3,
            container_id: Uuid::nil(),
            stamp: 0,
            ty: WalRecordType::Put,
            payload: b"c".to_vec(),
        }));
        let err = replay_bytes(&data, "default").unwrap_err();
        assert!(matches!(err, StorageError::WalCorrupt(_)));

        let dir = tempdir().unwrap();
        let path = dir.path().join("0000000000000001.wal");
        std::fs::write(&path, &data).unwrap();
        let dest = quarantine_sync(&path).unwrap();
        assert!(!path.exists());
        assert!(dest
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains(".corrupt."));
        assert!(dest.exists());
    }
}

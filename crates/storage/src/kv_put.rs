//! Persist container puts through DriveWal when a drive is open (003↔013 seam).

use crate::error::StorageError;
use crate::wal::{DriveWal, WalRecordType};
use spacestorage_types::{AckKind, StorageModeChoice, WriteAck};
use uuid::Uuid;

/// Append a put record and return a placement-ready `WriteAck` labelled for `mode`.
pub async fn put_durable(
    wal: &DriveWal,
    container_id: Uuid,
    stamp: u64,
    key: &[u8],
    value: &[u8],
    mode: StorageModeChoice,
    data_key: Option<&[u8; 32]>,
) -> Result<(u64, WriteAck), StorageError> {
    let mut payload = Vec::with_capacity(4 + key.len() + value.len());
    payload.extend_from_slice(&(key.len() as u32).to_le_bytes());
    payload.extend_from_slice(key);
    payload.extend_from_slice(value);
    let ack = wal
        .append_durable(container_id, stamp, WalRecordType::Put, payload, data_key)
        .await?;
    let labelled = WriteAck::from_wal(
        ack.replica.clone(),
        ack.drive_id.clone(),
        ack.lsn,
        ack.kind,
        mode,
        wal.durable_lsn(),
        "",
    );
    debug_assert!(
        !(matches!(mode, StorageModeChoice::Persistent | StorageModeChoice::Hybrid)
            && matches!(labelled.kind, AckKind::Durable))
            || wal.durable_lsn() >= ack.lsn
    );
    Ok((ack.lsn, labelled))
}

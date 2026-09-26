//! In-domain fanout helpers (leaderless source-domain coordinator path).
//!
//! Wire types live in [`crate::rpc`]; placement owns quorum math. These helpers
//! keep the internode crate aligned with FR-045: no Raft, durable flag on ACKs.

use crate::rpc::{FanoutRead, FanoutWrite, RpcAck};

/// Local crash-durable write ack (after replication `append_durable` / fsync).
pub fn local_write_ack(durable: bool) -> RpcAck {
    RpcAck {
        ok: true,
        error: None,
        durable,
    }
}

/// Memory / non-durable ack — MUST NOT satisfy persistent/hybrid write levels.
pub fn local_memory_ack() -> RpcAck {
    RpcAck {
        ok: true,
        error: None,
        durable: false,
    }
}

pub fn summarize_write(w: &FanoutWrite) -> String {
    format!(
        "fanout_write container={} bytes={}",
        w.container_id,
        w.value.len()
    )
}

pub fn summarize_read(r: &FanoutRead) -> String {
    format!("fanout_read container={}", r.container_id)
}

/// Count only crash-durable successful acks (in-domain filter is caller's job).
pub fn count_durable_ok(acks: &[RpcAck]) -> u32 {
    acks.iter().filter(|a| a.ok && a.durable).count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durable_acks_counted_memory_ignored() {
        let acks = vec![
            local_write_ack(true),
            local_memory_ack(),
            local_write_ack(true),
        ];
        assert_eq!(count_durable_ok(&acks), 2);
    }
}

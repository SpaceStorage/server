//! Handler-local `INFO` counters (`017` US4 T051/T055). One shared [`RedisStats`]
//! per handler backs the `clients` / `stats` / `commandstats` sections.
//!
//! These are the handler's own real counters (not a second statistics store that
//! shadows the product's observability — constitution X): the handler records
//! connections, commands, key hits/misses, and expired-key purges as they happen,
//! so `INFO` reports truth rather than a stock-Redis-looking template.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// Per-command usage: total calls and total wall-clock microseconds spent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CmdStat {
    pub calls: u64,
    pub usec: u64,
}

/// A plain, cloneable read of the atomic counters for one `INFO stats` render.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RedisStatsSnapshot {
    pub total_connections_received: u64,
    pub total_commands_processed: u64,
    pub expired_keys: u64,
    pub keyspace_hits: u64,
    pub keyspace_misses: u64,
    pub connected_clients: u64,
}

/// Shared, lock-light handler counters (`017` US4). Cloned via `Arc`.
#[derive(Debug, Default)]
pub struct RedisStats {
    total_connections_received: AtomicU64,
    total_commands_processed: AtomicU64,
    expired_keys: AtomicU64,
    keyspace_hits: AtomicU64,
    keyspace_misses: AtomicU64,
    connected_clients: AtomicU64,
    /// Per-command usage, keyed by the lower-cased command name.
    commands: Mutex<BTreeMap<String, CmdStat>>,
}

impl RedisStats {
    pub fn new() -> Self {
        Self::default()
    }

    /// A connection was accepted: count it and raise the live gauge.
    pub fn connection_opened(&self) {
        self.connected_clients.fetch_add(1, Ordering::Relaxed);
        self.total_connections_received
            .fetch_add(1, Ordering::Relaxed);
    }

    /// A connection ended: lower the live gauge (saturating at 0).
    pub fn connection_closed(&self) {
        let _ = self
            .connected_clients
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                Some(v.saturating_sub(1))
            });
    }

    /// Record one processed command with its measured elapsed microseconds.
    pub fn record_command(&self, command: &str, usec: u64) {
        self.total_commands_processed.fetch_add(1, Ordering::Relaxed);
        let mut map = self.commands.lock().expect("redis stats");
        let entry = map
            .entry(command.to_ascii_lowercase())
            .or_default();
        entry.calls = entry.calls.saturating_add(1);
        entry.usec = entry.usec.saturating_add(usec);
    }

    pub fn record_hit(&self) {
        self.keyspace_hits.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_miss(&self) {
        self.keyspace_misses.fetch_add(1, Ordering::Relaxed);
    }

    /// An expired key was purged on read.
    pub fn record_expired(&self) {
        self.expired_keys.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> RedisStatsSnapshot {
        RedisStatsSnapshot {
            total_connections_received: self.total_connections_received.load(Ordering::Relaxed),
            total_commands_processed: self.total_commands_processed.load(Ordering::Relaxed),
            expired_keys: self.expired_keys.load(Ordering::Relaxed),
            keyspace_hits: self.keyspace_hits.load(Ordering::Relaxed),
            keyspace_misses: self.keyspace_misses.load(Ordering::Relaxed),
            connected_clients: self.connected_clients.load(Ordering::Relaxed),
        }
    }

    /// Per-command usage sorted by command name: `(name, calls, usec)`.
    pub fn command_stats(&self) -> Vec<(String, u64, u64)> {
        let map = self.commands.lock().expect("redis stats");
        map.iter()
            .map(|(name, stat)| (name.clone(), stat.calls, stat.usec))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_move_and_command_stats_sort() {
        let s = RedisStats::new();
        s.connection_opened();
        s.connection_opened();
        s.connection_closed();
        s.record_command("GET", 10);
        s.record_command("get", 5);
        s.record_command("SET", 7);
        s.record_hit();
        s.record_miss();
        s.record_expired();

        let snap = s.snapshot();
        assert_eq!(snap.total_connections_received, 2);
        assert_eq!(snap.connected_clients, 1);
        assert_eq!(snap.total_commands_processed, 3);
        assert_eq!(snap.keyspace_hits, 1);
        assert_eq!(snap.keyspace_misses, 1);
        assert_eq!(snap.expired_keys, 1);

        let stats = s.command_stats();
        assert_eq!(
            stats,
            vec![
                ("get".to_string(), 2, 15),
                ("set".to_string(), 1, 7),
            ]
        );
    }

    #[test]
    fn connection_closed_saturates_at_zero() {
        let s = RedisStats::new();
        s.connection_closed();
        assert_eq!(s.snapshot().connected_clients, 0);
    }
}

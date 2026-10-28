//! `08` Raft leader-election series (FR-016 / T067).

use spacestorage_observability::{help_for, LabelSet, Metrics, DURATION_BUCKETS};
use std::time::Duration;

/// Emit election success / error / duration with `node` and `raft_group` labels.
pub fn record_election(
    metrics: &Metrics,
    node: &str,
    raft_group: &str,
    ok: bool,
    duration: Duration,
) {
    let mut labels = LabelSet::new();
    let _ = labels.insert("node", node);
    let _ = labels.insert("raft_group", raft_group);
    if ok {
        metrics.registry().inc(
            "spacestorage_raft_leader_elections_total",
            labels.clone(),
            help_for("spacestorage_raft_leader_elections_total"),
            1,
        );
    } else {
        metrics.registry().inc(
            "spacestorage_raft_leader_election_errors_total",
            labels.clone(),
            help_for("spacestorage_raft_leader_election_errors_total"),
            1,
        );
    }
    metrics.registry().observe(
        "spacestorage_raft_leader_election_duration_seconds",
        labels,
        help_for("spacestorage_raft_leader_election_duration_seconds"),
        DURATION_BUCKETS,
        duration.as_secs_f64(),
    );
}

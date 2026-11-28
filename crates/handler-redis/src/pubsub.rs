//! In-process, namespace-scoped pub/sub bus (`017` US3 T046, data-model §5).
//!
//! The bus is shared by every connection of one [`crate::RedisHandler`] exactly
//! like [`crate::block::BlockRegistry`]. It lives in `handler-redis` (rather
//! than `node`, which data-model §5 names) because the serve loop must reach it
//! and `handler-redis` may not depend on `node`; this sibling module is that
//! process-local bus. It is **never durable**: no message, channel, or pattern
//! is written to the store or the `013` WAL, and a new handler starts with an
//! empty bus. Delivery is confined to the connection's own namespace.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Mutex, MutexGuard};

use bytes::Bytes;
use tokio::sync::mpsc::UnboundedSender;

use crate::session::ConnectionId;

/// One out-of-band frame pushed to a subscriber connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushFrame {
    /// `["message", channel, payload]`.
    Message { channel: Bytes, payload: Bytes },
    /// `["pmessage", pattern, channel, payload]`.
    PMessage {
        pattern: Bytes,
        channel: Bytes,
        payload: Bytes,
    },
}

/// Per-connection subscription state inside the bus.
#[derive(Debug)]
struct ConnSub {
    tx: Option<UnboundedSender<PushFrame>>,
    channels: BTreeSet<Bytes>,
    patterns: BTreeSet<Bytes>,
}

impl ConnSub {
    fn new() -> Self {
        Self {
            tx: None,
            channels: BTreeSet::new(),
            patterns: BTreeSet::new(),
        }
    }

    fn total(&self) -> usize {
        self.channels.len() + self.patterns.len()
    }
}

/// Process-local pub/sub bus keyed by namespace then connection id.
#[derive(Debug, Default)]
pub struct PubSubBus {
    inner: Mutex<HashMap<String, HashMap<ConnectionId, ConnSub>>>,
}

impl PubSubBus {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, HashMap<ConnectionId, ConnSub>>> {
        // A poisoned lock only means another connection panicked mid-update; the
        // map is still structurally sound, so recover rather than propagate.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Register a connection's push channel (idempotent; keeps any existing
    /// channel/pattern state).
    pub fn attach(&self, ns: &str, id: ConnectionId, tx: UnboundedSender<PushFrame>) {
        let mut inner = self.lock();
        let conn = inner
            .entry(ns.to_string())
            .or_default()
            .entry(id)
            .or_insert_with(ConnSub::new);
        conn.tx = Some(tx);
    }

    /// Drop a connection and, when it was the namespace's last, the namespace.
    pub fn detach(&self, ns: &str, id: ConnectionId) {
        let mut inner = self.lock();
        let empty = if let Some(map) = inner.get_mut(ns) {
            map.remove(&id);
            map.is_empty()
        } else {
            false
        };
        if empty {
            inner.remove(ns);
        }
    }

    /// Subscribe to `channels`; returns this connection's new total subscription
    /// count (channels + patterns).
    pub fn subscribe(&self, ns: &str, id: ConnectionId, channels: &[Bytes]) -> usize {
        let mut inner = self.lock();
        let conn = inner
            .entry(ns.to_string())
            .or_default()
            .entry(id)
            .or_insert_with(ConnSub::new);
        for c in channels {
            conn.channels.insert(c.clone());
        }
        conn.total()
    }

    /// Unsubscribe from `channels`. `None` removes all channels. The returned
    /// list is one `(channel, remaining_total)` per confirmation; a `None`
    /// channel is the Redis nil-channel shape (`["unsubscribe", nil, n]`) when
    /// there was no channel to enumerate.
    pub fn unsubscribe(
        &self,
        ns: &str,
        id: ConnectionId,
        channels: Option<&[Bytes]>,
    ) -> Vec<(Option<Bytes>, usize)> {
        let mut inner = self.lock();
        let Some(conn) = inner.get_mut(ns).and_then(|m| m.get_mut(&id)) else {
            return match channels {
                Some(chs) => chs.iter().map(|c| (Some(c.clone()), 0)).collect(),
                None => vec![(None, 0)],
            };
        };
        match channels {
            Some(chs) => {
                let mut out = Vec::with_capacity(chs.len());
                for c in chs {
                    conn.channels.remove(c);
                    out.push((Some(c.clone()), conn.total()));
                }
                out
            }
            None => {
                let current: Vec<Bytes> = conn.channels.iter().cloned().collect();
                let mut out = Vec::new();
                if current.is_empty() {
                    out.push((None, conn.total()));
                } else {
                    for c in current {
                        conn.channels.remove(&c);
                        out.push((Some(c), conn.total()));
                    }
                }
                out
            }
        }
    }

    /// Subscribe to `patterns`; returns the new total subscription count.
    pub fn psubscribe(&self, ns: &str, id: ConnectionId, patterns: &[Bytes]) -> usize {
        let mut inner = self.lock();
        let conn = inner
            .entry(ns.to_string())
            .or_default()
            .entry(id)
            .or_insert_with(ConnSub::new);
        for p in patterns {
            conn.patterns.insert(p.clone());
        }
        conn.total()
    }

    /// Unsubscribe from `patterns`; `None` removes all patterns. Shape matches
    /// [`Self::unsubscribe`].
    pub fn punsubscribe(
        &self,
        ns: &str,
        id: ConnectionId,
        patterns: Option<&[Bytes]>,
    ) -> Vec<(Option<Bytes>, usize)> {
        let mut inner = self.lock();
        let Some(conn) = inner.get_mut(ns).and_then(|m| m.get_mut(&id)) else {
            return match patterns {
                Some(ps) => ps.iter().map(|p| (Some(p.clone()), 0)).collect(),
                None => vec![(None, 0)],
            };
        };
        match patterns {
            Some(ps) => {
                let mut out = Vec::with_capacity(ps.len());
                for p in ps {
                    conn.patterns.remove(p);
                    out.push((Some(p.clone()), conn.total()));
                }
                out
            }
            None => {
                let current: Vec<Bytes> = conn.patterns.iter().cloned().collect();
                let mut out = Vec::new();
                if current.is_empty() {
                    out.push((None, conn.total()));
                } else {
                    for p in current {
                        conn.patterns.remove(&p);
                        out.push((Some(p), conn.total()));
                    }
                }
                out
            }
        }
    }

    /// Publish `payload` on `channel` within `ns`; returns the number of
    /// matching subscriptions (channel + glob-matching patterns) on this
    /// process. Channel subscribers get [`PushFrame::Message`]; each matching
    /// pattern gets [`PushFrame::PMessage`].
    pub fn publish(&self, ns: &str, channel: &Bytes, payload: &Bytes) -> usize {
        let inner = self.lock();
        let Some(map) = inner.get(ns) else {
            return 0;
        };
        let mut delivered = 0usize;
        for conn in map.values() {
            if conn.channels.contains(channel) {
                if let Some(tx) = &conn.tx {
                    let _ = tx.send(PushFrame::Message {
                        channel: channel.clone(),
                        payload: payload.clone(),
                    });
                }
                delivered += 1;
            }
            for pat in &conn.patterns {
                if pattern_matches(pat, channel) {
                    if let Some(tx) = &conn.tx {
                        let _ = tx.send(PushFrame::PMessage {
                            pattern: pat.clone(),
                            channel: channel.clone(),
                            payload: payload.clone(),
                        });
                    }
                    delivered += 1;
                }
            }
        }
        delivered
    }

    /// Active channels in `ns` (optionally filtered by a glob `pattern`),
    /// deduplicated and sorted — `PUBSUB CHANNELS`.
    pub fn channels(&self, ns: &str, pattern: Option<&str>) -> Vec<Bytes> {
        let inner = self.lock();
        let Some(map) = inner.get(ns) else {
            return Vec::new();
        };
        let mut set = BTreeSet::new();
        for conn in map.values() {
            for c in &conn.channels {
                let keep = match pattern {
                    Some(p) => pattern_matches(p.as_bytes(), c),
                    None => true,
                };
                if keep {
                    set.insert(c.clone());
                }
            }
        }
        set.into_iter().collect()
    }

    /// Subscriber count per requested channel — `PUBSUB NUMSUB`.
    pub fn numsub(&self, ns: &str, channels: &[Bytes]) -> Vec<(Bytes, usize)> {
        let inner = self.lock();
        let Some(map) = inner.get(ns) else {
            return channels.iter().map(|c| (c.clone(), 0)).collect();
        };
        channels
            .iter()
            .map(|c| {
                let n = map
                    .values()
                    .filter(|conn| conn.channels.contains(c))
                    .count();
                (c.clone(), n)
            })
            .collect()
    }

    /// Total pattern subscriptions in `ns` — `PUBSUB NUMPAT`.
    pub fn numpat(&self, ns: &str) -> usize {
        let inner = self.lock();
        inner
            .get(ns)
            .map(|m| m.values().map(|c| c.patterns.len()).sum())
            .unwrap_or(0)
    }

    /// This connection's total subscription count (channels + patterns).
    pub fn count(&self, ns: &str, id: ConnectionId) -> usize {
        let inner = self.lock();
        inner
            .get(ns)
            .and_then(|m| m.get(&id))
            .map(ConnSub::total)
            .unwrap_or(0)
    }

    /// This connection's `(channels, patterns)` mirror.
    pub fn subscription_state(
        &self,
        ns: &str,
        id: ConnectionId,
    ) -> (BTreeSet<Bytes>, BTreeSet<Bytes>) {
        let inner = self.lock();
        match inner.get(ns).and_then(|m| m.get(&id)) {
            Some(c) => (c.channels.clone(), c.patterns.clone()),
            None => (BTreeSet::new(), BTreeSet::new()),
        }
    }
}

/// Redis glob matching for `PSUBSCRIBE`/`PUBSUB CHANNELS`: `*`, `?`, and
/// `[...]` character classes (with `^`/`!` negation and `a-z` ranges). Binary
/// safe; runs on bytes.
pub(crate) fn pattern_matches(pat: &[u8], s: &[u8]) -> bool {
    match_here(pat, s)
}

fn class_match(pat: &[u8], mut i: usize, c: u8) -> Option<(bool, usize)> {
    // pat[i] == b'['
    i += 1;
    let mut negate = false;
    if i < pat.len() && (pat[i] == b'^' || pat[i] == b'!') {
        negate = true;
        i += 1;
    }
    let mut matched = false;
    let mut first = true;
    while i < pat.len() {
        if pat[i] == b']' && !first {
            return Some((matched ^ negate, i + 1));
        }
        first = false;
        if i + 2 < pat.len() && pat[i + 1] == b'-' && pat[i + 2] != b']' {
            let (lo, hi) = (pat[i].min(pat[i + 2]), pat[i].max(pat[i + 2]));
            if c >= lo && c <= hi {
                matched = true;
            }
            i += 3;
        } else {
            if pat[i] == c {
                matched = true;
            }
            i += 1;
        }
    }
    None // unterminated class
}

fn match_here(pat: &[u8], s: &[u8]) -> bool {
    if pat.is_empty() {
        return s.is_empty();
    }
    match pat[0] {
        b'*' => {
            let mut i = 1;
            while i < pat.len() && pat[i] == b'*' {
                i += 1;
            }
            let rest = &pat[i..];
            if rest.is_empty() {
                return true;
            }
            (0..=s.len()).any(|k| match_here(rest, &s[k..]))
        }
        b'?' => !s.is_empty() && match_here(&pat[1..], &s[1..]),
        b'[' => match s.first().copied().and_then(|c| class_match(pat, 0, c)) {
            Some((ok, next)) => ok && match_here(&pat[next..], &s[1..]),
            None => {
                // Empty subject or malformed class: Redis treats `[` literally.
                !s.is_empty() && s[0] == b'[' && match_here(&pat[1..], &s[1..])
            }
        },
        b'\\' if pat.len() >= 2 => {
            !s.is_empty() && s[0] == pat[1] && match_here(&pat[2..], &s[1..])
        }
        c => !s.is_empty() && s[0] == c && match_here(&pat[1..], &s[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_forms() {
        assert!(pattern_matches(b"news.*", b"news.tech"));
        assert!(!pattern_matches(b"news.*", b"sports"));
        assert!(pattern_matches(b"n?ws", b"news"));
        assert!(pattern_matches(b"c[ae]t", b"cat"));
        assert!(pattern_matches(b"c[ae]t", b"cet"));
        assert!(!pattern_matches(b"c[ae]t", b"cot"));
        assert!(pattern_matches(b"c[^x]t", b"cat"));
        assert!(!pattern_matches(b"c[^x]t", b"cxt"));
        assert!(pattern_matches(b"c[a-z]t", b"cut"));
        assert!(!pattern_matches(b"c[a-z]t", b"c1t"));
        assert!(pattern_matches(b"*", b""));
        assert!(pattern_matches(b"a*b*c", b"axxbyyc"));
    }

    #[test]
    fn bus_is_namespace_keyed() {
        let bus = PubSubBus::new();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        bus.attach("a", ConnectionId(1), tx);
        let ch = Bytes::from_static(b"ch");
        let payload = Bytes::from_static(b"p");
        assert_eq!(bus.subscribe("a", ConnectionId(1), &[ch.clone()]), 1);
        // Same channel in another namespace does not deliver.
        assert_eq!(bus.publish("b", &ch, &payload), 0);
        assert!(rx.try_recv().is_err());
        // Same namespace delivers and counts.
        assert_eq!(bus.publish("a", &ch, &payload), 1);
        assert!(matches!(rx.try_recv(), Ok(PushFrame::Message { .. })));
    }
}

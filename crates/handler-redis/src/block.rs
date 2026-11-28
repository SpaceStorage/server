//! Connection-local blocking state (`017` T010, data-model §9). A blocking
//! command parks its connection task on a `tokio` notification (never a
//! worker-thread sleep — constitution II).

use std::time::{Duration, Instant};

use bytes::Bytes;

use crate::session::ConnectionId;

/// Which blocking family parked the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    BlPop,
    BrPop,
    BrPopLpush,
    BlMove,
    BzPopMin,
    BzPopMax,
    XRead,
    XReadGroup,
}

/// Parsed `XREAD`/`XREADGROUP … BLOCK` specification (`017` T036). The sync
/// dispatcher handles the non-blocking form; only the BLOCK form parks.
#[derive(Debug, Clone)]
pub struct XReadSpec {
    /// Stream keys, in order.
    pub keys: Vec<Bytes>,
    /// Raw id token per key (`$`, `>`, `0`, `1-0`); `$` is resolved once at
    /// park time by [`crate::commands::stream::resolve_dollar`].
    pub ids: Vec<String>,
    pub count: Option<usize>,
    /// `(group, consumer)` for `XREADGROUP`; `None` for `XREAD`.
    pub group: Option<(Bytes, Bytes)>,
    pub noack: bool,
}

/// A parsed blocking command awaiting data (`017` T027, data-model §9). Carried
/// from the async serve loop, which owns the wait; `dispatch` stays synchronous.
#[derive(Debug, Clone)]
pub struct BlockRequest {
    pub kind: BlockKind,
    /// Keys watched in order (`BLPOP`/`BRPOP`/`BZPOPMIN`/`BZPOPMAX`); one element
    /// for `BRPOPLPUSH`/`BLMOVE`.
    pub keys: Vec<Bytes>,
    /// Client timeout; `None` = client sent `0` (stock indefinite unless
    /// `max_block` is configured).
    pub timeout: Option<Duration>,
    /// `BRPOPLPUSH`/`BLMOVE` destination key.
    pub dest: Option<Bytes>,
    pub from_left: bool,
    pub to_left: bool,
    /// `XREAD`/`XREADGROUP … BLOCK` spec, else `None`.
    pub xread: Option<XReadSpec>,
}

/// Shared wake registry: every key write notifies parked connections. One per
/// process (owned by the handler), never a worker-thread sleep (constitution II).
#[derive(Debug, Default)]
pub struct BlockRegistry {
    notify: tokio::sync::Notify,
}

impl BlockRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Wake every parked connection; each re-checks its keys.
    pub fn wake(&self) {
        self.notify.notify_waiters();
    }

    /// Wait up to `d` for a wake, or a wake, whichever comes first.
    pub async fn wait(&self, d: Duration) {
        tokio::select! {
            _ = tokio::time::sleep(d) => {}
            _ = self.notify.notified() => {}
        }
    }
}

/// Parse a blocking verb into a [`BlockRequest`]. `None` when the verb is not a
/// blocking form or the arity/shape is invalid (the caller then falls through to
/// the synchronous dispatcher, which reports the protocol error).
pub fn parse_block(upper: &str, args: &[&str]) -> Option<BlockRequest> {
    match upper {
        "BLPOP" | "BRPOP" => {
            if args.len() < 2 {
                return None;
            }
            let timeout = parse_timeout(args.last()?)?;
            let keys = args[..args.len() - 1]
                .iter()
                .map(|k| Bytes::from(k.as_bytes().to_vec()))
                .collect();
            Some(BlockRequest {
                kind: if upper == "BLPOP" {
                    BlockKind::BlPop
                } else {
                    BlockKind::BrPop
                },
                keys,
                timeout,
                dest: None,
                from_left: true,
                to_left: false,
                xread: None,
            })
        }
        "BRPOPLPUSH" => {
            if args.len() != 3 {
                return None;
            }
            let timeout = parse_timeout(args[2])?;
            Some(BlockRequest {
                kind: BlockKind::BrPopLpush,
                keys: vec![Bytes::from(args[0].as_bytes().to_vec())],
                timeout,
                dest: Some(Bytes::from(args[1].as_bytes().to_vec())),
                from_left: false,
                to_left: true,
                xread: None,
            })
        }
        "BLMOVE" => {
            if args.len() != 5 {
                return None;
            }
            let from_left = match args[2].to_ascii_uppercase().as_str() {
                "LEFT" => true,
                "RIGHT" => false,
                _ => return None,
            };
            let to_left = match args[3].to_ascii_uppercase().as_str() {
                "LEFT" => true,
                "RIGHT" => false,
                _ => return None,
            };
            let timeout = parse_timeout(args[4])?;
            Some(BlockRequest {
                kind: BlockKind::BlMove,
                keys: vec![Bytes::from(args[0].as_bytes().to_vec())],
                timeout,
                dest: Some(Bytes::from(args[1].as_bytes().to_vec())),
                from_left,
                to_left,
                xread: None,
            })
        }
        "BZPOPMIN" | "BZPOPMAX" => {
            if args.len() < 2 {
                return None;
            }
            let timeout = parse_timeout(args.last()?)?;
            let keys = args[..args.len() - 1]
                .iter()
                .map(|k| Bytes::from(k.as_bytes().to_vec()))
                .collect();
            Some(BlockRequest {
                kind: if upper == "BZPOPMIN" {
                    BlockKind::BzPopMin
                } else {
                    BlockKind::BzPopMax
                },
                keys,
                timeout,
                dest: None,
                from_left: true,
                to_left: false,
                xread: None,
            })
        }
        "XREAD" | "XREADGROUP" => parse_stream_block(upper, args),
        _ => None,
    }
}

/// Parse `XREAD`/`XREADGROUP … BLOCK` into a [`BlockRequest`]. Returns `None`
/// (falls through to sync dispatch) when there is no `BLOCK` option or the shape
/// is invalid.
fn parse_stream_block(upper: &str, args: &[&str]) -> Option<BlockRequest> {
    let mut count = None;
    let mut block_ms: Option<u64> = None;
    let mut group: Option<(Bytes, Bytes)> = None;
    let mut noack = false;
    let mut i = 0;
    while i < args.len() {
        let opt = args[i].to_ascii_uppercase();
        match opt.as_str() {
            "COUNT" => {
                count = args.get(i + 1).and_then(|s| s.parse::<usize>().ok());
                i += 2;
            }
            "BLOCK" => {
                block_ms = args.get(i + 1).and_then(|s| s.parse::<u64>().ok());
                i += 2;
            }
            "GROUP" => {
                let (g, c) = (args.get(i + 1)?, args.get(i + 2)?);
                group = Some((
                    Bytes::from(g.as_bytes().to_vec()),
                    Bytes::from(c.as_bytes().to_vec()),
                ));
                i += 3;
            }
            "NOACK" => {
                noack = true;
                i += 1;
            }
            "STREAMS" => {
                let rest = &args[i + 1..];
                if rest.is_empty() || rest.len() % 2 != 0 {
                    return None;
                }
                let half = rest.len() / 2;
                let keys: Vec<Bytes> = rest[..half]
                    .iter()
                    .map(|k| Bytes::from(k.as_bytes().to_vec()))
                    .collect();
                let ids = rest[half..].iter().map(|s| s.to_string()).collect();
                let is_group = upper == "XREADGROUP";
                // `GROUP` is required for XREADGROUP and unknown for XREAD.
                if is_group && group.is_none() {
                    return None;
                }
                if !is_group && group.is_some() {
                    return None;
                }
                // Only a BLOCK form parks; otherwise the sync dispatcher runs it.
                let block = block_ms?;
                let timeout = if block == 0 {
                    None
                } else {
                    Some(Duration::from_millis(block))
                };
                return Some(BlockRequest {
                    kind: if is_group {
                        BlockKind::XReadGroup
                    } else {
                        BlockKind::XRead
                    },
                    keys: keys.clone(),
                    timeout,
                    dest: None,
                    from_left: true,
                    to_left: false,
                    xread: Some(XReadSpec {
                        keys,
                        ids,
                        count,
                        group,
                        noack,
                    }),
                });
            }
            _ => return None,
        }
    }
    None
}

/// `Some(None)` = timeout 0 (indefinite); `None` = not a valid number.
fn parse_timeout(s: &str) -> Option<Option<Duration>> {
    let secs: f64 = s.parse().ok()?;
    if !secs.is_finite() || secs < 0.0 {
        return None;
    }
    if secs == 0.0 {
        Some(None)
    } else {
        Some(Some(Duration::from_secs_f64(secs)))
    }
}

/// At most one per connection. Dropped on reply, timeout, disconnect, or drain.
#[derive(Debug, Clone)]
pub struct BlockingWait {
    pub connection: ConnectionId,
    pub keys: Vec<Bytes>,
    pub kind: BlockKind,
    /// `None` only when `max_block` is unset **and** the client timeout is 0.
    pub deadline: Option<Instant>,
}

/// FR-006 effective timeout. `client_timeout` `None` means the client sent
/// `0` (stock indefinite). A configured `max_block` overrides `0` and caps
/// longer explicit timeouts.
pub fn effective_timeout(
    client_timeout: Option<Duration>,
    max_block: Option<Duration>,
) -> Option<Duration> {
    match (client_timeout, max_block) {
        (None, None) => None,
        (None, Some(m)) => Some(m),
        (Some(t), None) => Some(t),
        (Some(t), Some(m)) => Some(t.min(m)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_timeout_rules() {
        let five = Duration::from_secs(5);
        let cap = Duration::from_secs(300);
        // 0 + unset => indefinite
        assert_eq!(effective_timeout(None, None), None);
        // 0 + cap => cap
        assert_eq!(effective_timeout(None, Some(cap)), Some(cap));
        // explicit + unset => explicit
        assert_eq!(effective_timeout(Some(five), None), Some(five));
        // explicit shorter than cap => explicit
        assert_eq!(effective_timeout(Some(five), Some(cap)), Some(five));
        // explicit longer than cap => cap
        let long = Duration::from_secs(600);
        assert_eq!(effective_timeout(Some(long), Some(cap)), Some(cap));
    }
}

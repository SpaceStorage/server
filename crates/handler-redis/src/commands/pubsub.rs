//! Redis `pubsub` family (`017` US3 T047–T048). Subscribe/unsubscribe/publish
//! and `PUBSUB` introspection over the process-local, namespace-scoped
//! [`crate::pubsub::PubSubBus`].
//!
//! Sharded forms (`SSUBSCRIBE`/`SUNSUBSCRIBE`/`SPUBLISH`) share the **same**
//! single-node bus as the normal forms: this dialect is one process, so there is
//! no separate shard index and `SPUBLISH` reaches a normal subscriber (and vice
//! versa). Delivery is confined to the connection's namespace, and nothing here
//! is ever written to the store or the `013` WAL.

use bytes::Bytes;
use spacestorage_types::NamespaceName;

use crate::render;
use crate::session::Subscription;

use super::{RedisReply, SessionState};

/// Which subscription set a verb addresses.
#[derive(Clone, Copy)]
enum Kind {
    /// `SUBSCRIBE`/`UNSUBSCRIBE` — channel set, `subscribe` keyword.
    Normal,
    /// `PSUBSCRIBE`/`PUNSUBSCRIBE` — pattern set, `psubscribe` keyword.
    Pattern,
    /// `SSUBSCRIBE`/`SUNSUBSCRIBE` — channel set, `ssubscribe` keyword.
    Sharded,
}

impl Kind {
    fn sub_keyword(self) -> &'static str {
        match self {
            Kind::Normal => "subscribe",
            Kind::Pattern => "psubscribe",
            Kind::Sharded => "ssubscribe",
        }
    }

    fn unsub_keyword(self) -> &'static str {
        match self {
            Kind::Normal => "unsubscribe",
            Kind::Pattern => "punsubscribe",
            Kind::Sharded => "sunsubscribe",
        }
    }
}

/// The pub/sub verbs. Returns `None` for any other command so the caller falls
/// through to the data families.
pub fn dispatch(session: &mut SessionState, upper: &str, args: &[&str]) -> Option<RedisReply> {
    match upper {
        "SUBSCRIBE" => Some(subscribe(session, args, Kind::Normal)),
        "PSUBSCRIBE" => Some(subscribe(session, args, Kind::Pattern)),
        "SSUBSCRIBE" => Some(subscribe(session, args, Kind::Sharded)),
        "UNSUBSCRIBE" => Some(unsubscribe(session, args, Kind::Normal)),
        "PUNSUBSCRIBE" => Some(unsubscribe(session, args, Kind::Pattern)),
        "SUNSUBSCRIBE" => Some(unsubscribe(session, args, Kind::Sharded)),
        "PUBLISH" => Some(publish(session, args, "publish")),
        "SPUBLISH" => Some(publish(session, args, "spublish")),
        "PUBSUB" => Some(pubsub_introspection(session, args)),
        _ => None,
    }
}

fn bytes(arg: &str) -> Bytes {
    Bytes::from(arg.as_bytes().to_vec())
}

/// Register `args` on the bus and render one `[keyword, name, count]`
/// confirmation per item; the count is this connection's running total.
fn subscribe(session: &mut SessionState, args: &[&str], kind: Kind) -> RedisReply {
    if args.is_empty() {
        return render::arity(kind.sub_keyword());
    }
    let ns = session.namespace.clone();
    let id = session.connection_id;
    let keyword = kind.sub_keyword();
    let mut frames = Vec::with_capacity(args.len());
    for arg in args {
        let item = bytes(arg);
        let n = match kind {
            Kind::Pattern => session
                .pubsub
                .psubscribe(&ns, id, std::slice::from_ref(&item)),
            Kind::Normal | Kind::Sharded => session
                .pubsub
                .subscribe(&ns, id, std::slice::from_ref(&item)),
        };
        frames.push(confirmation(keyword, Some(item), n));
    }
    refresh_subscription(session);
    RedisReply::Frames(frames)
}

/// Remove `args` (all when empty) and render one confirmation per item; the
/// nil-channel shape (`[keyword, nil, n]`) appears when there was nothing to
/// enumerate.
fn unsubscribe(session: &mut SessionState, args: &[&str], kind: Kind) -> RedisReply {
    let ns = session.namespace.clone();
    let id = session.connection_id;
    let requested: Option<Vec<Bytes>> = if args.is_empty() {
        None
    } else {
        Some(args.iter().map(|a| bytes(a)).collect())
    };
    let entries = match kind {
        Kind::Pattern => session.pubsub.punsubscribe(&ns, id, requested.as_deref()),
        Kind::Normal | Kind::Sharded => {
            session.pubsub.unsubscribe(&ns, id, requested.as_deref())
        }
    };
    let keyword = kind.unsub_keyword();
    let frames = entries
        .into_iter()
        .map(|(name, n)| confirmation(keyword, name, n))
        .collect();
    refresh_subscription(session);
    RedisReply::Frames(frames)
}

fn confirmation(keyword: &str, name: Option<Bytes>, count: usize) -> RedisReply {
    RedisReply::Array(vec![
        RedisReply::Bulk(Some(keyword.as_bytes().to_vec())),
        RedisReply::Bulk(name.map(|b| b.to_vec())),
        RedisReply::Integer(count as i64),
    ])
}

/// `PUBLISH`/`SPUBLISH`: deliver on the process-local bus and return the number
/// of matching subscriptions (channels + glob-matching patterns) on this
/// process.
fn publish(session: &SessionState, args: &[&str], cmd: &str) -> RedisReply {
    let [channel, payload] = args else {
        return render::arity(cmd);
    };
    let n = session
        .pubsub
        .publish(&session.namespace, &bytes(channel), &bytes(payload));
    RedisReply::Integer(n as i64)
}

/// `PUBSUB CHANNELS [pattern]` / `NUMSUB [channel ...]` / `NUMPAT`.
fn pubsub_introspection(session: &SessionState, args: &[&str]) -> RedisReply {
    let Some(sub) = args.first() else {
        return render::unknown_subcommand();
    };
    match sub.to_ascii_uppercase().as_str() {
        "CHANNELS" => {
            if args.len() > 2 {
                return render::arity("pubsub|channels");
            }
            let pattern = args.get(1).copied();
            let chans = session.pubsub.channels(&session.namespace, pattern);
            RedisReply::Array(
                chans
                    .into_iter()
                    .map(|c| RedisReply::Bulk(Some(c.to_vec())))
                    .collect(),
            )
        }
        "NUMSUB" => {
            let channels: Vec<Bytes> = args[1..].iter().map(|a| bytes(a)).collect();
            let pairs = session.pubsub.numsub(&session.namespace, &channels);
            let mut out = Vec::with_capacity(pairs.len() * 2);
            for (c, n) in pairs {
                out.push(RedisReply::Bulk(Some(c.to_vec())));
                out.push(RedisReply::Integer(n as i64));
            }
            RedisReply::Array(out)
        }
        "NUMPAT" => {
            if args.len() != 1 {
                return render::arity("pubsub|numpat");
            }
            RedisReply::Integer(session.pubsub.numpat(&session.namespace) as i64)
        }
        _ => render::unknown_subcommand(),
    }
}

/// Mirror the bus's view of this connection into `SessionState.subscriptions`.
/// `None` here means command mode (no channels and no patterns).
fn refresh_subscription(session: &mut SessionState) {
    let ns = session.namespace.clone();
    let id = session.connection_id;
    let (channels, patterns) = session.pubsub.subscription_state(&ns, id);
    if channels.is_empty() && patterns.is_empty() {
        session.subscriptions = None;
    } else {
        session.subscriptions = Some(Subscription {
            connection: id,
            namespace: NamespaceName::new(ns),
            channels,
            patterns,
        });
    }
}

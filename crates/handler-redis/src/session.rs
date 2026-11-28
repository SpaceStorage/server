//! Assembled connection session (`017` T008, data-model §10). One per client
//! connection: credential-bound namespace, the key-space container, and the
//! connection-scoped transaction / subscribe / blocking state.

use std::collections::BTreeSet;

use bytes::Bytes;
use spacestorage_query::QueryOptions;
use spacestorage_types::NamespaceName;

use crate::block::BlockingWait;
use crate::commands::txn::TransactionSession;

/// Identity of a client connection (RESP2 `CLIENT ID`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConnectionId(pub u64);

/// Subscribe-mode state (`017` data-model §5). `Some` ⇒ subscribe mode.
#[derive(Debug, Clone)]
pub struct Subscription {
    pub connection: ConnectionId,
    pub namespace: NamespaceName,
    pub channels: BTreeSet<Bytes>,
    pub patterns: BTreeSet<Bytes>,
}

impl Subscription {
    pub fn new(connection: ConnectionId, namespace: NamespaceName) -> Self {
        Self {
            connection,
            namespace,
            channels: BTreeSet::new(),
            patterns: BTreeSet::new(),
        }
    }

    /// Delivers only when the message namespace equals the subscription's.
    pub fn accepts(&self, message: &PubSubMessage) -> bool {
        message.namespace == self.namespace
    }

    pub fn has_channels(&self) -> bool {
        !self.channels.is_empty() || !self.patterns.is_empty()
    }
}

/// A namespace-scoped published message (process-local, never durable).
#[derive(Debug, Clone)]
pub struct PubSubMessage {
    pub namespace: NamespaceName,
    pub channel: Bytes,
    pub payload: Bytes,
}

/// The assembled per-connection session.
#[derive(Debug)]
pub struct RedisSession {
    pub namespace: NamespaceName,
    pub container: String,
    pub authenticated: bool,
    pub txn: TransactionSession,
    pub subscriptions: Option<Subscription>,
    pub blocking: Option<BlockingWait>,
    pub options: QueryOptions,
}

impl RedisSession {
    pub fn new(namespace: NamespaceName, container: impl Into<String>) -> Self {
        Self {
            namespace,
            container: container.into(),
            authenticated: false,
            txn: TransactionSession::default(),
            subscriptions: None,
            blocking: None,
            options: QueryOptions::default(),
        }
    }

    /// `subscriptions` `Some` ⇒ subscribe mode.
    pub fn in_subscribe_mode(&self) -> bool {
        self.subscriptions.is_some()
    }

    pub fn connection_id(&self) -> ConnectionId {
        self.blocking
            .as_ref()
            .map(|b| b.connection)
            .or_else(|| self.subscriptions.as_ref().map(|s| s.connection))
            .unwrap_or(ConnectionId(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_scoping() {
        let ns_a = NamespaceName::new("a");
        let ns_b = NamespaceName::new("b");
        let sub = Subscription::new(ConnectionId(1), ns_a.clone());
        assert!(sub.accepts(&PubSubMessage {
            namespace: ns_a,
            channel: Bytes::from_static(b"c"),
            payload: Bytes::from_static(b"p"),
        }));
        assert!(!sub.accepts(&PubSubMessage {
            namespace: ns_b,
            channel: Bytes::from_static(b"c"),
            payload: Bytes::from_static(b"p"),
        }));
    }

    #[test]
    fn subscribe_mode_flag() {
        let mut s = RedisSession::new(NamespaceName::new("demo"), "redis");
        assert!(!s.in_subscribe_mode());
        s.subscriptions = Some(Subscription::new(ConnectionId(7), s.namespace.clone()));
        assert!(s.in_subscribe_mode());
    }
}

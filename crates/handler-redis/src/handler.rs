//! Async RESP2 serve loop (Constitution II).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use bytes::BytesMut;
use spacestorage_compat::{DialectProfile, LimitServices};
use spacestorage_types::ContainerCatalog;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use tracing::debug;

use crate::block::{BlockKind, BlockRegistry, BlockRequest, effective_timeout, parse_block};
use crate::commands::{RedisReply, SessionState, dispatch, list, stream, zset};
use crate::pubsub::{PubSubBus, PushFrame};
use crate::resp::{RespError, RespValue, encode, try_decode};
use crate::session::ConnectionId;
use crate::stats::RedisStats;

/// Poll floor for a parked blocking command; a lost wake still re-checks within
/// this bound. Blocking waits live on the async task, never a worker-thread
/// sleep (constitution II).
const BLOCK_POLL: Duration = Duration::from_millis(50);

/// Process-unique connection ids for the pub/sub bus (`017` US3).
static NEXT_CONNECTION_ID: AtomicU64 = AtomicU64::new(1);

pub struct RedisHandler {
    pub catalog: Arc<RwLock<ContainerCatalog>>,
    pub expected_user: String,
    pub expected_secret: String,
    pub namespace: String,
    pub default_container: String,
    pub limit_services: LimitServices,
    pub tenancy: Option<Arc<spacestorage_tenancy::Tenancy>>,
    /// Active dialect profile (`017` T012); default first-binary.
    pub profile: DialectProfile,
    /// Shared across every connection so a write on one wakes a blocked other.
    pub blocking: Arc<BlockRegistry>,
    /// Per-stream-key total-order leadership lease (`017` T038). The running node
    /// injects a `controlplane`-backed provider via [`RedisHandler::with_stream_leases`];
    /// the constructors install the dev/harness `single_node` fallback.
    pub stream_leases: Arc<dyn spacestorage_protocol_core::StreamLeaseProvider>,
    /// Process-local, namespace-scoped pub/sub bus (`017` US3), shared across
    /// every connection this handler serves.
    pub pubsub: Arc<PubSubBus>,
    /// Handler-local `INFO` counters (`017` US4), shared across connections.
    pub stats: Arc<RedisStats>,
    /// Configured blocking cap (`017` US4); `CONFIG GET max_block` reports it and
    /// [`crate::block::effective_timeout`] applies it in `run_block`.
    pub max_block: Option<Duration>,
    /// Node facts for `INFO` (`017` US4). The default standalone provider omits
    /// facts the handler cannot see; the running node injects real facts.
    pub node_info: Arc<dyn spacestorage_protocol_core::NodeInfoProvider>,
}

impl RedisHandler {
    pub fn with_demo(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self::with_limits(catalog, LimitServices::built_in())
    }

    pub fn with_limits(
        catalog: Arc<RwLock<ContainerCatalog>>,
        limit_services: LimitServices,
    ) -> Self {
        Self {
            catalog,
            expected_user: "demo".into(),
            expected_secret: "demo".into(),
            namespace: "demo".into(),
            default_container: "redis".into(),
            limit_services,
            tenancy: None,
            profile: DialectProfile::FirstBinary,
            blocking: Arc::new(BlockRegistry::new()),
            // Dev / harness fallback: the sole writer auto-grants epoch 1. A
            // running node replaces this via `with_stream_leases` with the
            // `controlplane`-backed provider.
            stream_leases: Arc::new(crate::lease::StreamLeaseAuthority::single_node()),
            pubsub: Arc::new(PubSubBus::new()),
            stats: Arc::new(RedisStats::new()),
            max_block: None,
            node_info: Arc::new(spacestorage_protocol_core::StandaloneNodeInfo::new()),
        }
    }

    /// `017` T038: inject the node's `controlplane`-backed leadership-lease
    /// provider. In-process tests keep the dev/harness `single_node` fallback.
    pub fn with_stream_leases(
        mut self,
        leases: Arc<dyn spacestorage_protocol_core::StreamLeaseProvider>,
    ) -> Self {
        self.stream_leases = leases;
        self
    }

    /// `017` US4 T051: inject the node's real facts for `INFO`
    /// (`server`/`memory`/`replication`). A running node passes its own provider;
    /// in-process tests keep the standalone default.
    pub fn with_node_info(
        mut self,
        info: Arc<dyn spacestorage_protocol_core::NodeInfoProvider>,
    ) -> Self {
        self.node_info = info;
        self
    }

    /// `017` US4 T054: configure the `max_block` cap for blocking commands.
    pub fn with_max_block(mut self, max_block: Option<Duration>) -> Self {
        self.max_block = max_block;
        self
    }

    pub fn with_tenancy(mut self, tenancy: Arc<spacestorage_tenancy::Tenancy>) -> Self {
        self.tenancy = Some(tenancy);
        self
    }

    /// `017` T012: set the active dialect profile for every connection.
    pub fn with_profile(mut self, profile: DialectProfile) -> Self {
        self.profile = profile;
        self
    }

    pub async fn serve<S>(&self, mut stream: S, cancel: CancellationToken)
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        let peeked = match spacestorage_protocol_core::peek_signature(
            &mut stream,
            spacestorage_protocol_core::ProtocolFamily::Redis,
            spacestorage_protocol_core::default_timeout(),
        )
        .await
        {
            Ok(buf) => buf,
            Err((_err, refusal)) => {
                let _ = stream.write_all(&refusal.body).await;
                let _ = stream.shutdown().await;
                return;
            }
        };

        let connection_id = ConnectionId(NEXT_CONNECTION_ID.fetch_add(1, Ordering::Relaxed));
        // `017` US4: count the accepted connection and lower the gauge on every
        // exit path (the guard drops with the serve loop).
        self.stats.connection_opened();
        let _conn = ConnGuard {
            stats: Arc::clone(&self.stats),
        };
        let (push_tx, mut push_rx) = tokio::sync::mpsc::unbounded_channel::<PushFrame>();
        self.pubsub
            .attach(&self.namespace, connection_id, push_tx);
        // Detach on every exit path (disconnect, cancel, protocol error).
        let _detach = DetachGuard {
            bus: Arc::clone(&self.pubsub),
            namespace: self.namespace.clone(),
            connection_id,
        };

        let mut session = SessionState {
            catalog: Arc::clone(&self.catalog),
            ttls: Arc::new(RwLock::new(Default::default())),
            authenticated: false,
            namespace: self.namespace.clone(),
            container: self.default_container.clone(),
            password_ok: false,
            expected_secret: self.expected_secret.clone(),
            expected_user: self.expected_user.clone(),
            limit_services: self.limit_services.clone(),
            principal_hold: None,
            tenancy: self.tenancy.clone(),
            connection_quota: None,
            principal: None,
            profile: self.profile,
            block_registry: Arc::clone(&self.blocking),
            stream_leases: Arc::clone(&self.stream_leases),
            txn: Default::default(),
            connection_id,
            subscriptions: None,
            pubsub: Arc::clone(&self.pubsub),
            stats: Arc::clone(&self.stats),
            node_info: Arc::clone(&self.node_info),
            max_block: self.max_block,
            client_name: None,
        };

        let mut buf = BytesMut::from(peeked.as_slice());
        let mut read_buf = vec![0u8; 4096];

        loop {
            // Drain peeked / buffered frames before blocking on read.
            loop {
                match try_decode(&mut buf) {
                    Ok(None) => break,
                    Ok(Some(frame)) => {
                        let (reply, close) = match handle_frame(&mut session, frame) {
                            FrameOutcome::Reply(r) => (r, false),
                            FrameOutcome::Block(req) => {
                                (run_block(&mut session, req).await, false)
                            }
                            FrameOutcome::ReplyThenClose(r) => (r, true),
                        };
                        if write_reply(&mut stream, &reply).await.is_err() {
                            return;
                        }
                        if close {
                            // `017` US4 `QUIT`: flush `+OK`, then close.
                            let _ = stream.shutdown().await;
                            return;
                        }
                    }
                    Err(RespError::Incomplete) => break,
                    Err(e) => {
                        debug!(error = %e, "resp decode error");
                        let _ = write_reply(
                            &mut stream,
                            &RedisReply::Error(format!("ERR Protocol error: {e}")),
                        )
                        .await;
                        return;
                    }
                }
            }

            tokio::select! {
                _ = cancel.cancelled() => break,
                push = push_rx.recv() => {
                    if let Some(frame) = push {
                        if write_push(&mut stream, &frame).await.is_err() {
                            return;
                        }
                    }
                }
                n = stream.read(&mut read_buf) => {
                    match n {
                        Ok(0) | Err(_) => break,
                        Ok(n) => buf.extend_from_slice(&read_buf[..n]),
                    }
                }
            }
        }
        // Drop session → PrincipalHold::drop releases principal slot.
    }
}

/// Removes a connection from the pub/sub bus when its serve loop ends.
struct DetachGuard {
    bus: Arc<PubSubBus>,
    namespace: String,
    connection_id: ConnectionId,
}

impl Drop for DetachGuard {
    fn drop(&mut self) {
        self.bus.detach(&self.namespace, self.connection_id);
    }
}

/// Lowers the `connected_clients` gauge when a serve loop ends (`017` US4).
struct ConnGuard {
    stats: Arc<RedisStats>,
}

impl Drop for ConnGuard {
    fn drop(&mut self) {
        self.stats.connection_closed();
    }
}

/// Encode one out-of-band pub/sub push frame (binary-safe).
async fn write_push<S: AsyncWrite + Unpin>(
    stream: &mut S,
    frame: &PushFrame,
) -> std::io::Result<()> {
    let value = match frame {
        PushFrame::Message { channel, payload } => RespValue::Array(vec![
            RespValue::BulkString(Some(b"message".to_vec())),
            RespValue::BulkString(Some(channel.to_vec())),
            RespValue::BulkString(Some(payload.to_vec())),
        ]),
        PushFrame::PMessage {
            pattern,
            channel,
            payload,
        } => RespValue::Array(vec![
            RespValue::BulkString(Some(b"pmessage".to_vec())),
            RespValue::BulkString(Some(pattern.to_vec())),
            RespValue::BulkString(Some(channel.to_vec())),
            RespValue::BulkString(Some(payload.to_vec())),
        ]),
    };
    let mut out = BytesMut::new();
    encode(&value, &mut out);
    stream.write_all(&out).await?;
    stream.flush().await
}

fn handle_frame(session: &mut SessionState, frame: RespValue) -> FrameOutcome {
    let args = match frame_to_args(frame) {
        Ok(a) => a,
        Err(e) => return FrameOutcome::Reply(RedisReply::Error(e)),
    };
    if args.is_empty() {
        return FrameOutcome::Reply(RedisReply::Error("ERR empty command".into()));
    }
    let cmd = args[0].clone();
    let rest: Vec<&str> = args[1..].iter().map(|s| s.as_str()).collect();
    // `017` US4 T054: `QUIT` replies `+OK` (from `server::dispatch`) and then
    // closes the connection; the serve loop owns the close.
    if cmd.eq_ignore_ascii_case("QUIT") {
        return FrameOutcome::ReplyThenClose(dispatch(session, &cmd, &rest));
    }
    // `017` T027: blocking verbs park on the async serve loop's registry, after
    // authentication and only on the complete-product profile.
    if session.profile == DialectProfile::CompleteProduct && session.authenticated {
        if let Some(req) = parse_block(&cmd.to_ascii_uppercase(), &rest) {
            return FrameOutcome::Block(req);
        }
    }
    FrameOutcome::Reply(dispatch(session, &cmd, &rest))
}

/// A decoded frame's outcome: an immediate reply, a parked blocking command, or
/// a reply followed by connection close (`QUIT`).
enum FrameOutcome {
    Reply(RedisReply),
    Block(BlockRequest),
    ReplyThenClose(RedisReply),
}

/// Drive a blocking command: return as soon as a key has data, else park until a
/// write wakes us; the client timeout (0 = indefinite) bounds the wait.
async fn run_block(session: &mut SessionState, mut req: BlockRequest) -> RedisReply {
    // Resolve `$` once at issue time (stock `XREAD BLOCK` semantics: `$` is the
    // last id present when the block was established, not on every re-check).
    stream::resolve_dollar(session, &mut req);
    // `017` US4: a configured `max_block` overrides a client `0` (indefinite)
    // and caps longer explicit timeouts (FR-006).
    let timeout = effective_timeout(req.timeout, session.max_block);
    let deadline = timeout.map(|t| Instant::now() + t);
    loop {
        if let Some(reply) = try_block(session, &req) {
            return reply;
        }
        let wait = match deadline {
            None => BLOCK_POLL,
            Some(d) => {
                let now = Instant::now();
                if now >= d {
                    // RESP2 null array (`*-1`) for every blocking form.
                    return RedisReply::NullArray;
                }
                (d - now).min(BLOCK_POLL)
            }
        };
        let registry = Arc::clone(&session.block_registry);
        registry.wait(wait).await;
    }
}

fn try_block(session: &mut SessionState, req: &BlockRequest) -> Option<RedisReply> {
    match req.kind {
        BlockKind::BlPop | BlockKind::BrPop => list::try_block_pop(session, req),
        BlockKind::BrPopLpush | BlockKind::BlMove => list::try_block_move(session, req),
        BlockKind::BzPopMin | BlockKind::BzPopMax => zset::try_block_pop(session, req),
        BlockKind::XRead | BlockKind::XReadGroup => stream::try_block(session, req),
    }
}

fn frame_to_args(frame: RespValue) -> Result<Vec<String>, String> {
    match frame {
        RespValue::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for it in items {
                out.push(frame_to_string(it)?);
            }
            Ok(out)
        }
        other => Ok(vec![frame_to_string(other)?]),
    }
}

fn frame_to_string(frame: RespValue) -> Result<String, String> {
    match frame {
        RespValue::BulkString(Some(b)) => String::from_utf8(b).map_err(|e| e.to_string()),
        RespValue::BulkString(None) => Ok(String::new()),
        RespValue::SimpleString(s) => Ok(s),
        RespValue::Integer(i) => Ok(i.to_string()),
        RespValue::Error(e) => Ok(e),
        RespValue::Array(_) => Err("ERR unexpected nested array".into()),
        RespValue::NullArray => Ok(String::new()),
    }
}

async fn write_reply<S: AsyncWrite + Unpin>(
    stream: &mut S,
    reply: &RedisReply,
) -> std::io::Result<()> {
    let mut out = BytesMut::new();
    match reply {
        // Independent frames are written back-to-back (subscribe confirmations,
        // subscribe-mode PING).
        RedisReply::Frames(items) => {
            for item in items {
                encode(&reply_to_value(item), &mut out);
            }
        }
        other => encode(&reply_to_value(other), &mut out),
    }
    stream.write_all(&out).await?;
    stream.flush().await
}

fn reply_to_value(reply: &RedisReply) -> RespValue {
    match reply {
        RedisReply::Ok => RespValue::SimpleString("OK".into()),
        RedisReply::Pong => RespValue::SimpleString("PONG".into()),
        RedisReply::Simple(s) => RespValue::SimpleString(s.clone()),
        RedisReply::Bulk(None) => RespValue::BulkString(None),
        RedisReply::Bulk(Some(b)) => RespValue::BulkString(Some(b.clone())),
        RedisReply::Integer(i) => RespValue::Integer(*i),
        RedisReply::Array(items) => RespValue::Array(items.iter().map(reply_to_value).collect()),
        // A nested `Frames` is not produced by the current command set; flatten
        // defensively into an array so it can never panic.
        RedisReply::Frames(items) => RespValue::Array(items.iter().map(reply_to_value).collect()),
        RedisReply::NullArray => RespValue::NullArray,
        RedisReply::Error(e) => RespValue::Error(e.clone()),
    }
}

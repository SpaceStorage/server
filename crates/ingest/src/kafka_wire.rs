//! Live Kafka broker I/O via `kafka-protocol` (no librdkafka / OpenSSL).
//!
//! Metadata + FindCoordinator + JoinGroup/SyncGroup (range assignor) + Heartbeat +
//! Fetch (per-partition leaders) + OffsetFetch/OffsetCommit (coordinator + generation)
//! for ingest ALO. FakeKafkaBroker remains for unit tests.

use bytes::{Bytes, BytesMut};
use kafka_protocol::{
    messages::{
        consumer_protocol_assignment::{
            ConsumerProtocolAssignment, TopicPartition as AssignedTopicPartition,
        },
        consumer_protocol_subscription::ConsumerProtocolSubscription,
        fetch_request::{FetchPartition, FetchTopic},
        find_coordinator_response::Coordinator,
        join_group_request::JoinGroupRequestProtocol,
        join_group_response::JoinGroupResponseMember,
        offset_commit_request::{OffsetCommitRequestPartition, OffsetCommitRequestTopic},
        offset_fetch_request::OffsetFetchRequestTopic,
        sync_group_request::SyncGroupRequestAssignment,
        ApiKey, FetchRequest, FetchResponse, FindCoordinatorRequest, FindCoordinatorResponse,
        GroupId, HeartbeatRequest, HeartbeatResponse, JoinGroupRequest, JoinGroupResponse,
        MetadataRequest, MetadataResponse, OffsetCommitRequest, OffsetCommitResponse,
        OffsetFetchRequest, OffsetFetchResponse, RequestHeader, ResponseHeader, SyncGroupRequest,
        SyncGroupResponse, TopicName,
    },
    protocol::{Decodable, Encodable, HeaderVersion, StrBytes},
    records::{Compression, Record, RecordBatchDecoder},
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::ClientConfig;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fs::File;
use std::io::{self, BufReader};
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;

use crate::declaration::KafkaTls;
use crate::kafka::KafkaMessage;
use crate::IngestError;

const CLIENT_ID: &str = "spacestorage-ingest";
/// Stable versions that use classic (non-compact) topic/partition layouts.
const FETCH_API: i16 = 11;
const METADATA_API: i16 = 12;
const OFFSET_FETCH_API: i16 = 7;
const OFFSET_COMMIT_API: i16 = 8;
const FIND_COORDINATOR_API: i16 = 3;
const JOIN_GROUP_API: i16 = 5;
const SYNC_GROUP_API: i16 = 3;
const HEARTBEAT_API: i16 = 3;
const PROTOCOL_TYPE: &str = "consumer";
const RANGE_PROTOCOL: &str = "range";
/// MEMBER_ID_REQUIRED — rejoin with broker-assigned member id.
const ERR_MEMBER_ID_REQUIRED: i16 = 79;
const ERR_UNKNOWN_MEMBER_ID: i16 = 3;
const ERR_ILLEGAL_GENERATION: i16 = 22;
const ERR_REBALANCE_IN_PROGRESS: i16 = 25;
const ERR_NOT_COORDINATOR: i16 = 16;
const ERR_NOT_COORDINATOR_V2: i16 = 27;
const ERR_GROUP_COORDINATOR_NOT_AVAILABLE: i16 = 15;
const CONSUMER_PROTOCOL_VERSION: i16 = 0;
const SESSION_TIMEOUT_MS: i32 = 30_000;

#[derive(Debug, thiserror::Error)]
pub enum KafkaWireError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("timeout")]
    Timeout,
    #[error("no brokers")]
    NoBrokers,
    #[error("tls: {0}")]
    Tls(String),
    #[error("rejoin_required")]
    RejoinRequired,
}

impl From<KafkaWireError> for String {
    fn from(e: KafkaWireError) -> Self {
        e.to_string()
    }
}

fn topic_name(s: &str) -> TopicName {
    TopicName(StrBytes::from_string(s.to_string()))
}

fn group_id(s: &str) -> GroupId {
    GroupId(StrBytes::from_string(s.to_string()))
}

fn needs_rejoin(code: i16) -> bool {
    matches!(
        code,
        ERR_UNKNOWN_MEMBER_ID
            | ERR_ILLEGAL_GENERATION
            | ERR_REBALANCE_IN_PROGRESS
            | ERR_NOT_COORDINATOR
            | ERR_NOT_COORDINATOR_V2
            | ERR_GROUP_COORDINATOR_NOT_AVAILABLE
    )
}

enum KafkaConn {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl AsyncRead for KafkaConn {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            KafkaConn::Plain(s) => Pin::new(s).poll_read(cx, buf),
            KafkaConn::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for KafkaConn {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            KafkaConn::Plain(s) => Pin::new(s).poll_write(cx, buf),
            KafkaConn::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            KafkaConn::Plain(s) => Pin::new(s).poll_flush(cx),
            KafkaConn::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            KafkaConn::Plain(s) => Pin::new(s).poll_shutdown(cx),
            KafkaConn::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }
}

/// Async Kafka client for one ingest declaration (topic + group).
pub struct LiveKafkaClient {
    brokers: Vec<String>,
    topic: String,
    group: String,
    plaintext: bool,
    tls: Option<KafkaTls>,
    tls_connector: parking_lot::Mutex<Option<TlsConnector>>,
    correlation: AtomicI32,
    next_offset: parking_lot::Mutex<BTreeMap<i32, i64>>,
    pending: parking_lot::Mutex<VecDeque<KafkaMessage>>,
    partitions: parking_lot::Mutex<Vec<i32>>,
    /// broker id → host:port
    brokers_by_id: parking_lot::Mutex<BTreeMap<i32, String>>,
    /// partition → leader broker id
    partition_leaders: parking_lot::Mutex<BTreeMap<i32, i32>>,
    /// Group member id assigned by the coordinator (JoinGroup).
    member_id: parking_lot::Mutex<String>,
    generation_id: parking_lot::Mutex<i32>,
    /// Host:port of the group coordinator (FindCoordinator), if known.
    coordinator: parking_lot::Mutex<Option<String>>,
}

impl LiveKafkaClient {
    pub fn new(brokers: Vec<String>, topic: impl Into<String>, group: impl Into<String>) -> Self {
        Self::with_transport(brokers, topic, group, true, None)
    }

    pub fn with_transport(
        brokers: Vec<String>,
        topic: impl Into<String>,
        group: impl Into<String>,
        plaintext: bool,
        tls: Option<KafkaTls>,
    ) -> Self {
        Self {
            brokers,
            topic: topic.into(),
            group: group.into(),
            plaintext,
            tls,
            tls_connector: parking_lot::Mutex::new(None),
            correlation: AtomicI32::new(1),
            next_offset: parking_lot::Mutex::new(BTreeMap::new()),
            pending: parking_lot::Mutex::new(VecDeque::new()),
            partitions: parking_lot::Mutex::new(vec![0]),
            brokers_by_id: parking_lot::Mutex::new(BTreeMap::new()),
            partition_leaders: parking_lot::Mutex::new(BTreeMap::new()),
            member_id: parking_lot::Mutex::new(String::new()),
            generation_id: parking_lot::Mutex::new(0),
            coordinator: parking_lot::Mutex::new(None),
        }
    }

    pub fn from_decl(
        brokers: Vec<String>,
        topic: impl Into<String>,
        group: impl Into<String>,
        plaintext: bool,
        tls: Option<KafkaTls>,
    ) -> Result<Self, IngestError> {
        if !plaintext {
            let Some(ref t) = tls else {
                return Err(IngestError::Invalid(
                    "tls required when plaintext is false".into(),
                ));
            };
            if t.certificate.contains("-----BEGIN") || t.key.contains("-----BEGIN") {
                return Err(IngestError::CertInline);
            }
        }
        Ok(Self::with_transport(brokers, topic, group, plaintext, tls))
    }

    pub async fn bootstrap(&self) -> Result<(), KafkaWireError> {
        if !self.plaintext {
            self.ensure_tls_connector().await?;
        }
        self.refresh_metadata().await?;
        self.find_coordinator().await?;
        self.join_and_sync().await?;
        self.load_committed_offsets().await?;
        Ok(())
    }

    /// Heartbeat with joined generation + member id (before session timeout).
    pub async fn heartbeat(&self) -> Result<(), KafkaWireError> {
        let mut stream = self.connect_coordinator().await?;
        let generation = *self.generation_id.lock();
        let member_id = self.member_id.lock().clone();
        let mut req = HeartbeatRequest::default();
        req.group_id = group_id(&self.group);
        req.generation_id = generation;
        req.member_id = StrBytes::from_string(member_id);
        let resp: HeartbeatResponse = self
            .roundtrip(&mut stream, ApiKey::Heartbeat, HEARTBEAT_API, req)
            .await?;
        if needs_rejoin(resp.error_code) {
            return Err(KafkaWireError::RejoinRequired);
        }
        if resp.error_code != 0 {
            return Err(KafkaWireError::Protocol(format!(
                "heartbeat error {}",
                resp.error_code
            )));
        }
        Ok(())
    }

    /// Re-find coordinator and rejoin (after coordinator / generation errors).
    pub async fn rejoin(&self) -> Result<(), KafkaWireError> {
        self.find_coordinator().await?;
        self.join_and_sync().await?;
        self.load_committed_offsets().await?;
        Ok(())
    }

    pub fn session_timeout(&self) -> Duration {
        Duration::from_millis(SESSION_TIMEOUT_MS as u64)
    }

    pub async fn refresh_metadata(&self) -> Result<(), KafkaWireError> {
        let mut stream = self.connect_bootstrap().await?;
        let mut req = MetadataRequest::default();
        req.topics = Some(vec![
            kafka_protocol::messages::metadata_request::MetadataRequestTopic::default()
                .with_name(Some(topic_name(&self.topic))),
        ]);
        let resp: MetadataResponse = self
            .roundtrip(&mut stream, ApiKey::Metadata, METADATA_API, req)
            .await?;
        let mut brokers = BTreeMap::new();
        for b in resp.brokers {
            let id: i32 = b.node_id.0;
            brokers.insert(id, format!("{}:{}", b.host.as_str(), b.port));
        }
        *self.brokers_by_id.lock() = brokers;

        let mut parts = Vec::new();
        let mut leaders = BTreeMap::new();
        for t in resp.topics {
            let name = t
                .name
                .as_ref()
                .map(|n| n.0.as_str().to_string())
                .unwrap_or_default();
            if name != self.topic {
                continue;
            }
            if t.error_code != 0 {
                return Err(KafkaWireError::Protocol(format!(
                    "metadata topic error {}",
                    t.error_code
                )));
            }
            for p in t.partitions {
                if p.error_code == 0 {
                    parts.push(p.partition_index);
                    let leader: i32 = p.leader_id.0;
                    leaders.insert(p.partition_index, leader);
                }
            }
        }
        if parts.is_empty() {
            parts.push(0);
        }
        parts.sort_unstable();
        *self.partitions.lock() = parts;
        *self.partition_leaders.lock() = leaders;
        Ok(())
    }

    pub async fn load_committed_offsets(&self) -> Result<(), KafkaWireError> {
        let mut stream = self.connect_coordinator().await?;
        let parts = self.partitions.lock().clone();
        let mut req = OffsetFetchRequest::default();
        req.group_id = group_id(&self.group);
        req.topics = Some(vec![OffsetFetchRequestTopic::default()
            .with_name(topic_name(&self.topic))
            .with_partition_indexes(parts.clone())]);
        let resp: OffsetFetchResponse = self
            .roundtrip(&mut stream, ApiKey::OffsetFetch, OFFSET_FETCH_API, req)
            .await?;
        if needs_rejoin(resp.error_code) {
            return Err(KafkaWireError::RejoinRequired);
        }
        let mut next = self.next_offset.lock();
        for t in resp.topics {
            for p in t.partitions {
                if p.error_code == 0 && p.committed_offset >= 0 {
                    next.insert(p.partition_index, p.committed_offset + 1);
                } else {
                    next.entry(p.partition_index).or_insert(0);
                }
            }
        }
        for p in parts {
            next.entry(p).or_insert(0);
        }
        Ok(())
    }

    pub async fn fetch_one(&self) -> Result<Option<KafkaMessage>, KafkaWireError> {
        if let Some(msg) = self.pending.lock().pop_front() {
            return Ok(Some(msg));
        }
        self.fetch_into_pending().await?;
        Ok(self.pending.lock().pop_front())
    }

    pub fn requeue(&self, msg: KafkaMessage) {
        self.pending.lock().push_front(msg);
    }

    pub async fn commit(&self, partition: i32, offset: i64) -> Result<(), KafkaWireError> {
        let mut stream = self.connect_coordinator().await?;
        let generation = *self.generation_id.lock();
        let member_id = self.member_id.lock().clone();
        let mut req = OffsetCommitRequest::default();
        req.group_id = group_id(&self.group);
        req.generation_id_or_member_epoch = generation;
        req.member_id = StrBytes::from_string(member_id);
        req.topics = vec![OffsetCommitRequestTopic::default()
            .with_name(topic_name(&self.topic))
            .with_partitions(vec![OffsetCommitRequestPartition::default()
                .with_partition_index(partition)
                .with_committed_offset(offset + 1)])];
        let resp: OffsetCommitResponse = self
            .roundtrip(&mut stream, ApiKey::OffsetCommit, OFFSET_COMMIT_API, req)
            .await?;
        for t in resp.topics {
            for p in t.partitions {
                if needs_rejoin(p.error_code) {
                    return Err(KafkaWireError::RejoinRequired);
                }
                if p.error_code != 0 {
                    return Err(KafkaWireError::Protocol(format!(
                        "offset commit error {}",
                        p.error_code
                    )));
                }
            }
        }
        self.next_offset.lock().insert(partition, offset + 1);
        Ok(())
    }

    async fn fetch_into_pending(&self) -> Result<(), KafkaWireError> {
        let parts = self.partitions.lock().clone();
        let offsets = self.next_offset.lock().clone();
        let leaders = self.partition_leaders.lock().clone();
        let brokers = self.brokers_by_id.lock().clone();

        // Group assigned partitions by metadata leader address.
        let mut by_leader: HashMap<String, Vec<i32>> = HashMap::new();
        for p in &parts {
            let addr = leaders
                .get(p)
                .and_then(|id| brokers.get(id).cloned())
                .or_else(|| self.brokers.first().cloned())
                .ok_or(KafkaWireError::NoBrokers)?;
            by_leader.entry(addr).or_default().push(*p);
        }

        for (addr, leader_parts) in by_leader {
            let mut stream = self.connect_addr(&addr).await?;
            let mut fetch_parts = Vec::new();
            for p in leader_parts {
                let off = offsets.get(&p).copied().unwrap_or(0);
                fetch_parts.push(
                    FetchPartition::default()
                        .with_partition(p)
                        .with_fetch_offset(off)
                        .with_partition_max_bytes(1_048_576),
                );
            }
            let mut req = FetchRequest::default();
            req.max_wait_ms = 250;
            req.min_bytes = 1;
            req.max_bytes = 1_048_576;
            req.topics = vec![FetchTopic::default()
                .with_topic(topic_name(&self.topic))
                .with_partitions(fetch_parts)];
            let resp: FetchResponse = self
                .roundtrip(&mut stream, ApiKey::Fetch, FETCH_API, req)
                .await?;
            let mut pending = self.pending.lock();
            let mut next = self.next_offset.lock();
            for t in resp.responses {
                let topic_name_s = t.topic.0.as_str().to_string();
                for p in t.partitions {
                    if p.error_code != 0 {
                        continue;
                    }
                    let Some(mut records_bytes) = p.records else {
                        continue;
                    };
                    let records = RecordBatchDecoder::decode_with_custom_compression(
                        &mut records_bytes,
                        None::<fn(&mut Bytes, Compression) -> anyhow::Result<Bytes>>,
                    )
                    .map_err(|e| KafkaWireError::Protocol(format!("record decode: {e}")))?;
                    for rec in records {
                        if rec.control {
                            continue;
                        }
                        let msg = record_to_message(&topic_name_s, p.partition_index, &rec);
                        next.insert(p.partition_index, msg.offset + 1);
                        pending.push_back(msg);
                    }
                }
            }
        }
        Ok(())
    }

    async fn find_coordinator(&self) -> Result<(), KafkaWireError> {
        let mut stream = self.connect_bootstrap().await?;
        let mut req = FindCoordinatorRequest::default();
        req.key = StrBytes::from_string(self.group.clone());
        req.key_type = 0; // group
        let resp: FindCoordinatorResponse = self
            .roundtrip(
                &mut stream,
                ApiKey::FindCoordinator,
                FIND_COORDINATOR_API,
                req,
            )
            .await?;
        if resp.error_code != 0 {
            return Err(KafkaWireError::Protocol(format!(
                "find coordinator error {}",
                resp.error_code
            )));
        }
        let host = if !resp.host.as_str().is_empty() {
            format!("{}:{}", resp.host.as_str(), resp.port)
        } else if let Some(c) = resp.coordinators.first() {
            coordinator_addr(c)?
        } else {
            return Err(KafkaWireError::Protocol("find coordinator empty".into()));
        };
        *self.coordinator.lock() = Some(host);
        Ok(())
    }

    async fn join_and_sync(&self) -> Result<(), KafkaWireError> {
        let topic_parts = self.partitions.lock().clone();
        let mut member_id = self.member_id.lock().clone();
        let join = loop {
            let resp = self.join_group(&member_id, &topic_parts).await?;
            if resp.error_code == ERR_MEMBER_ID_REQUIRED {
                member_id = resp.member_id.as_str().to_string();
                *self.member_id.lock() = member_id.clone();
                continue;
            }
            if resp.error_code != 0 {
                return Err(KafkaWireError::Protocol(format!(
                    "join group error {}",
                    resp.error_code
                )));
            }
            break resp;
        };
        *self.member_id.lock() = join.member_id.as_str().to_string();
        *self.generation_id.lock() = join.generation_id;
        let is_leader = join.leader.as_str() == join.member_id.as_str();
        let assignments = if is_leader {
            range_assign(&self.topic, &topic_parts, &join.members)?
        } else {
            Vec::new()
        };
        let sync = self
            .sync_group(join.member_id.as_str(), join.generation_id, assignments)
            .await?;
        if sync.error_code != 0 {
            return Err(KafkaWireError::Protocol(format!(
                "sync group error {}",
                sync.error_code
            )));
        }
        let assigned = decode_assignment(&sync.assignment)?;
        let mut parts = Vec::new();
        for tp in assigned.assigned_partitions {
            if tp.topic.0.as_str() == self.topic {
                parts.extend(tp.partitions);
            }
        }
        if parts.is_empty() {
            parts = topic_parts;
        }
        parts.sort_unstable();
        parts.dedup();
        *self.partitions.lock() = parts;
        Ok(())
    }

    async fn join_group(
        &self,
        member_id: &str,
        _topic_parts: &[i32],
    ) -> Result<JoinGroupResponse, KafkaWireError> {
        let mut stream = self.connect_coordinator().await?;
        let meta = encode_subscription(&self.topic)?;
        let mut req = JoinGroupRequest::default();
        req.group_id = group_id(&self.group);
        req.session_timeout_ms = SESSION_TIMEOUT_MS;
        req.rebalance_timeout_ms = 60_000;
        req.member_id = StrBytes::from_string(member_id.to_string());
        req.protocol_type = StrBytes::from_static_str(PROTOCOL_TYPE);
        req.protocols = vec![JoinGroupRequestProtocol::default()
            .with_name(StrBytes::from_static_str(RANGE_PROTOCOL))
            .with_metadata(meta)];
        self.roundtrip(&mut stream, ApiKey::JoinGroup, JOIN_GROUP_API, req)
            .await
    }

    async fn sync_group(
        &self,
        member_id: &str,
        generation: i32,
        assignments: Vec<SyncGroupRequestAssignment>,
    ) -> Result<SyncGroupResponse, KafkaWireError> {
        let mut stream = self.connect_coordinator().await?;
        let mut req = SyncGroupRequest::default();
        req.group_id = group_id(&self.group);
        req.generation_id = generation;
        req.member_id = StrBytes::from_string(member_id.to_string());
        req.assignments = assignments;
        self.roundtrip(&mut stream, ApiKey::SyncGroup, SYNC_GROUP_API, req)
            .await
    }

    async fn ensure_tls_connector(&self) -> Result<(), KafkaWireError> {
        if self.tls_connector.lock().is_some() {
            return Ok(());
        }
        let tls = self.tls.clone();
        let connector = tokio::task::spawn_blocking(move || build_tls_connector(tls.as_ref()))
            .await
            .map_err(|e| KafkaWireError::Tls(e.to_string()))??;
        *self.tls_connector.lock() = Some(connector);
        Ok(())
    }

    async fn connect_bootstrap(&self) -> Result<KafkaConn, KafkaWireError> {
        if self.brokers.is_empty() {
            return Err(KafkaWireError::NoBrokers);
        }
        let mut last = None;
        for b in &self.brokers {
            match self.connect_addr(b).await {
                Ok(s) => return Ok(s),
                Err(e) => last = Some(e),
            }
        }
        Err(last.unwrap_or(KafkaWireError::NoBrokers))
    }

    async fn connect_coordinator(&self) -> Result<KafkaConn, KafkaWireError> {
        let addr = self.coordinator.lock().clone();
        if let Some(addr) = addr {
            if let Ok(s) = self.connect_addr(&addr).await {
                return Ok(s);
            }
        }
        self.connect_bootstrap().await
    }

    async fn connect_addr(&self, addr: &str) -> Result<KafkaConn, KafkaWireError> {
        let tcp = match timeout(Duration::from_secs(3), TcpStream::connect(addr)).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => return Err(KafkaWireError::Io(e)),
            Err(_) => return Err(KafkaWireError::Timeout),
        };
        if self.plaintext {
            return Ok(KafkaConn::Plain(tcp));
        }
        // TLS selected — never open a plaintext Kafka session.
        self.ensure_tls_connector().await?;
        let connector = self
            .tls_connector
            .lock()
            .clone()
            .ok_or_else(|| KafkaWireError::Tls("connector missing".into()))?;
        let host = addr.split(':').next().unwrap_or(addr);
        let server_name = ServerName::try_from(host.to_string())
            .map_err(|e| KafkaWireError::Tls(format!("server name: {e}")))?;
        let tls = timeout(Duration::from_secs(5), connector.connect(server_name, tcp))
            .await
            .map_err(|_| KafkaWireError::Timeout)?
            .map_err(|e| KafkaWireError::Tls(e.to_string()))?;
        Ok(KafkaConn::Tls(Box::new(tls)))
    }

    async fn roundtrip<Req, Resp>(
        &self,
        stream: &mut KafkaConn,
        api_key: ApiKey,
        version: i16,
        body: Req,
    ) -> Result<Resp, KafkaWireError>
    where
        Req: Encodable + HeaderVersion,
        Resp: Decodable + HeaderVersion,
    {
        let corr = self.correlation.fetch_add(1, Ordering::Relaxed);
        let mut header = RequestHeader::default();
        header.request_api_key = api_key as i16;
        header.request_api_version = version;
        header.correlation_id = corr;
        header.client_id = Some(StrBytes::from_static_str(CLIENT_ID));

        let mut bytes = BytesMut::new();
        header
            .encode(&mut bytes, Req::header_version(version))
            .map_err(|e| KafkaWireError::Protocol(e.to_string()))?;
        body.encode(&mut bytes, version)
            .map_err(|e| KafkaWireError::Protocol(e.to_string()))?;
        let size = bytes.len() as i32;
        stream.write_all(&size.to_be_bytes()).await?;
        stream.write_all(&bytes).await?;

        let msg_size = {
            let mut len_buf = [0u8; 4];
            stream.read_exact(&mut len_buf).await?;
            i32::from_be_bytes(len_buf) as usize
        };
        let mut buf = vec![0u8; msg_size];
        stream.read_exact(&mut buf).await?;
        let mut buf = Bytes::from(buf);
        let _hdr = ResponseHeader::decode(&mut buf, Resp::header_version(version))
            .map_err(|e| KafkaWireError::Protocol(e.to_string()))?;
        Resp::decode(&mut buf, version).map_err(|e| KafkaWireError::Protocol(e.to_string()))
    }
}

fn build_tls_connector(tls: Option<&KafkaTls>) -> Result<TlsConnector, KafkaWireError> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let builder = ClientConfig::builder().with_root_certificates(roots);
    let cfg = if let Some(tls) = tls {
        let certs = load_certs(Path::new(&tls.certificate))?;
        let key = load_key(Path::new(&tls.key))?;
        builder
            .with_client_auth_cert(certs, key)
            .map_err(|e| KafkaWireError::Tls(e.to_string()))?
    } else {
        builder.with_no_client_auth()
    };
    Ok(TlsConnector::from(Arc::new(cfg)))
}

fn load_certs(path: &Path) -> Result<Vec<CertificateDer<'static>>, KafkaWireError> {
    let f = File::open(path).map_err(|e| KafkaWireError::Tls(format!("{}: {e}", path.display())))?;
    let mut reader = BufReader::new(f);
    rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| KafkaWireError::Tls(format!("cert {}: {e}", path.display())))
}

fn load_key(path: &Path) -> Result<PrivateKeyDer<'static>, KafkaWireError> {
    let f = File::open(path).map_err(|e| KafkaWireError::Tls(format!("{}: {e}", path.display())))?;
    let mut reader = BufReader::new(f);
    rustls_pemfile::private_key(&mut reader)
        .map_err(|e| KafkaWireError::Tls(format!("key {}: {e}", path.display())))?
        .ok_or_else(|| KafkaWireError::Tls(format!("key {}: empty", path.display())))
}

fn record_to_message(topic: &str, partition: i32, rec: &Record) -> KafkaMessage {
    KafkaMessage {
        key: rec.key.as_ref().map(|k| k.to_vec()),
        value: rec.value.as_ref().map(|v| v.to_vec()).unwrap_or_default(),
        topic: topic.to_string(),
        partition,
        offset: rec.offset,
        broker_ts: Some(rec.timestamp.to_string()),
    }
}

fn coordinator_addr(c: &Coordinator) -> Result<String, KafkaWireError> {
    if c.error_code != 0 {
        return Err(KafkaWireError::Protocol(format!(
            "coordinator error {}",
            c.error_code
        )));
    }
    Ok(format!("{}:{}", c.host.as_str(), c.port))
}

fn encode_subscription(topic: &str) -> Result<Bytes, KafkaWireError> {
    let sub = ConsumerProtocolSubscription::default()
        .with_topics(vec![StrBytes::from_string(topic.to_string())]);
    let mut body = BytesMut::new();
    sub.encode(&mut body, CONSUMER_PROTOCOL_VERSION)
        .map_err(|e| KafkaWireError::Protocol(e.to_string()))?;
    let mut out = BytesMut::new();
    out.extend_from_slice(&CONSUMER_PROTOCOL_VERSION.to_be_bytes());
    out.extend_from_slice(&body);
    Ok(out.freeze())
}

fn encode_assignment(topic: &str, partitions: &[i32]) -> Result<Bytes, KafkaWireError> {
    let assign = ConsumerProtocolAssignment::default().with_assigned_partitions(vec![
        AssignedTopicPartition::default()
            .with_topic(topic_name(topic))
            .with_partitions(partitions.to_vec()),
    ]);
    let mut body = BytesMut::new();
    assign
        .encode(&mut body, CONSUMER_PROTOCOL_VERSION)
        .map_err(|e| KafkaWireError::Protocol(e.to_string()))?;
    let mut out = BytesMut::new();
    out.extend_from_slice(&CONSUMER_PROTOCOL_VERSION.to_be_bytes());
    out.extend_from_slice(&body);
    Ok(out.freeze())
}

fn decode_assignment(raw: &Bytes) -> Result<ConsumerProtocolAssignment, KafkaWireError> {
    if raw.is_empty() {
        return Ok(ConsumerProtocolAssignment::default());
    }
    if raw.len() < 2 {
        return Err(KafkaWireError::Protocol("assignment too short".into()));
    }
    let mut rest = raw.slice(2..);
    ConsumerProtocolAssignment::decode(&mut rest, CONSUMER_PROTOCOL_VERSION)
        .map_err(|e| KafkaWireError::Protocol(format!("assignment decode: {e}")))
}

/// Classic Kafka range assignor: sort members, give each a contiguous partition range.
pub fn range_assign_partitions(partitions: &[i32], member_count: usize) -> Vec<Vec<i32>> {
    if member_count == 0 {
        return Vec::new();
    }
    let mut parts = partitions.to_vec();
    parts.sort_unstable();
    let n = parts.len();
    let base = n / member_count;
    let rem = n % member_count;
    let mut out = Vec::with_capacity(member_count);
    let mut idx = 0usize;
    for i in 0..member_count {
        let take = base + if i < rem { 1 } else { 0 };
        out.push(parts[idx..idx + take].to_vec());
        idx += take;
    }
    out
}

fn range_assign(
    topic: &str,
    partitions: &[i32],
    members: &[JoinGroupResponseMember],
) -> Result<Vec<SyncGroupRequestAssignment>, KafkaWireError> {
    let mut ids: Vec<String> = members
        .iter()
        .map(|m| m.member_id.as_str().to_string())
        .collect();
    ids.sort();
    let ranges = range_assign_partitions(partitions, ids.len());
    let mut out = Vec::with_capacity(ids.len());
    for (member_id, parts) in ids.into_iter().zip(ranges) {
        let assignment = encode_assignment(topic, &parts)?;
        out.push(
            SyncGroupRequestAssignment::default()
                .with_member_id(StrBytes::from_string(member_id))
                .with_assignment(assignment),
        );
    }
    Ok(out)
}

/// Encode a Metadata request frame (length-prefixed) for unit tests.
pub fn encode_metadata_request_bytes(topic: &str) -> Result<Vec<u8>, KafkaWireError> {
    let mut req = MetadataRequest::default();
    req.topics = Some(vec![
        kafka_protocol::messages::metadata_request::MetadataRequestTopic::default()
            .with_name(Some(topic_name(topic))),
    ]);
    let mut header = RequestHeader::default();
    header.request_api_key = ApiKey::Metadata as i16;
    header.request_api_version = METADATA_API;
    header.correlation_id = 1;
    header.client_id = Some(StrBytes::from_static_str(CLIENT_ID));
    let mut bytes = BytesMut::new();
    header
        .encode(
            &mut bytes,
            MetadataRequest::header_version(METADATA_API),
        )
        .map_err(|e| KafkaWireError::Protocol(e.to_string()))?;
    req.encode(&mut bytes, METADATA_API)
        .map_err(|e| KafkaWireError::Protocol(e.to_string()))?;
    let mut out = Vec::with_capacity(4 + bytes.len());
    out.extend_from_slice(&(bytes.len() as i32).to_be_bytes());
    out.extend_from_slice(&bytes);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_request_has_length_prefix() {
        let bytes = encode_metadata_request_bytes("logs").unwrap();
        assert!(bytes.len() > 4);
        let len = i32::from_be_bytes(bytes[0..4].try_into().unwrap()) as usize;
        assert_eq!(len, bytes.len() - 4);
    }

    #[test]
    fn range_assignor_splits_contiguous() {
        let ranges = range_assign_partitions(&[0, 1, 2, 3, 4], 3);
        assert_eq!(ranges, vec![vec![0, 1], vec![2, 3], vec![4]]);
    }

    #[test]
    fn subscription_roundtrip_prefix() {
        let bytes = encode_subscription("logs").unwrap();
        assert!(bytes.len() > 2);
        assert_eq!(i16::from_be_bytes([bytes[0], bytes[1]]), 0);
    }

    #[test]
    fn rejects_inline_pem_in_from_decl() {
        let res = LiveKafkaClient::from_decl(
            vec!["127.0.0.1:9093".into()],
            "t",
            "g",
            false,
            Some(KafkaTls {
                certificate: "-----BEGIN CERTIFICATE-----\nMII\n-----END CERTIFICATE-----".into(),
                key: "/tmp/key.pem".into(),
            }),
        );
        assert!(res.is_err());
        assert_eq!(res.err().unwrap().code(), "ingest_cert_inline");
    }
}

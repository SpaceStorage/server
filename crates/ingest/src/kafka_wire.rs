//! Live Kafka broker I/O via `kafka-protocol` (no librdkafka / OpenSSL).
//!
//! Metadata + FindCoordinator + JoinGroup/SyncGroup (range assignor) + Fetch +
//! OffsetFetch + OffsetCommit for ingest ALO. FakeKafkaBroker remains for unit tests.

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
        GroupId, JoinGroupRequest, JoinGroupResponse, MetadataRequest, MetadataResponse,
        OffsetCommitRequest, OffsetCommitResponse, OffsetFetchRequest, OffsetFetchResponse,
        RequestHeader, ResponseHeader, SyncGroupRequest, SyncGroupResponse, TopicName,
    },
    protocol::{Decodable, Encodable, HeaderVersion, StrBytes},
    records::{Compression, Record, RecordBatchDecoder},
};
use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::kafka::KafkaMessage;

const CLIENT_ID: &str = "spacestorage-ingest";
/// Stable versions that use classic (non-compact) topic/partition layouts.
const FETCH_API: i16 = 11;
const METADATA_API: i16 = 12;
const OFFSET_FETCH_API: i16 = 7;
const OFFSET_COMMIT_API: i16 = 8;
const FIND_COORDINATOR_API: i16 = 3;
const JOIN_GROUP_API: i16 = 5;
const SYNC_GROUP_API: i16 = 3;
const PROTOCOL_TYPE: &str = "consumer";
const RANGE_PROTOCOL: &str = "range";
/// MEMBER_ID_REQUIRED — rejoin with broker-assigned member id.
const ERR_MEMBER_ID_REQUIRED: i16 = 79;
const CONSUMER_PROTOCOL_VERSION: i16 = 0;

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

/// Async Kafka client for one ingest declaration (topic + group).
pub struct LiveKafkaClient {
    brokers: Vec<String>,
    topic: String,
    group: String,
    correlation: AtomicI32,
    next_offset: parking_lot::Mutex<BTreeMap<i32, i64>>,
    pending: parking_lot::Mutex<VecDeque<KafkaMessage>>,
    partitions: parking_lot::Mutex<Vec<i32>>,
    /// Group member id assigned by the coordinator (JoinGroup).
    member_id: parking_lot::Mutex<String>,
    generation_id: parking_lot::Mutex<i32>,
    /// Host:port of the group coordinator (FindCoordinator), if known.
    coordinator: parking_lot::Mutex<Option<String>>,
}

impl LiveKafkaClient {
    pub fn new(brokers: Vec<String>, topic: impl Into<String>, group: impl Into<String>) -> Self {
        Self {
            brokers,
            topic: topic.into(),
            group: group.into(),
            correlation: AtomicI32::new(1),
            next_offset: parking_lot::Mutex::new(BTreeMap::new()),
            pending: parking_lot::Mutex::new(VecDeque::new()),
            partitions: parking_lot::Mutex::new(vec![0]),
            member_id: parking_lot::Mutex::new(String::new()),
            generation_id: parking_lot::Mutex::new(0),
            coordinator: parking_lot::Mutex::new(None),
        }
    }

    pub async fn bootstrap(&self) -> Result<(), KafkaWireError> {
        self.refresh_metadata().await?;
        self.find_coordinator().await?;
        self.join_and_sync().await?;
        self.load_committed_offsets().await?;
        Ok(())
    }

    pub async fn refresh_metadata(&self) -> Result<(), KafkaWireError> {
        let mut stream = self.connect_first().await?;
        let mut req = MetadataRequest::default();
        req.topics = Some(vec![
            kafka_protocol::messages::metadata_request::MetadataRequestTopic::default()
                .with_name(Some(topic_name(&self.topic))),
        ]);
        let resp: MetadataResponse = self
            .roundtrip(&mut stream, ApiKey::Metadata, METADATA_API, req)
            .await?;
        let mut parts = Vec::new();
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
                }
            }
        }
        if parts.is_empty() {
            parts.push(0);
        }
        parts.sort_unstable();
        *self.partitions.lock() = parts;
        Ok(())
    }

    pub async fn load_committed_offsets(&self) -> Result<(), KafkaWireError> {
        let mut stream = self.connect_first().await?;
        let parts = self.partitions.lock().clone();
        let mut req = OffsetFetchRequest::default();
        req.group_id = group_id(&self.group);
        req.topics = Some(vec![OffsetFetchRequestTopic::default()
            .with_name(topic_name(&self.topic))
            .with_partition_indexes(parts.clone())]);
        let resp: OffsetFetchResponse = self
            .roundtrip(&mut stream, ApiKey::OffsetFetch, OFFSET_FETCH_API, req)
            .await?;
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
        let mut stream = self.connect_first().await?;
        let mut req = OffsetCommitRequest::default();
        req.group_id = group_id(&self.group);
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
        let mut stream = self.connect_first().await?;
        let parts = self.partitions.lock().clone();
        let offsets = self.next_offset.lock().clone();
        let mut fetch_parts = Vec::new();
        for p in parts {
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
        Ok(())
    }


    async fn find_coordinator(&self) -> Result<(), KafkaWireError> {
        let mut stream = self.connect_first().await?;
        let mut req = FindCoordinatorRequest::default();
        req.key = StrBytes::from_string(self.group.clone());
        req.key_type = 0; // group
        let resp: FindCoordinatorResponse = self
            .roundtrip(&mut stream, ApiKey::FindCoordinator, FIND_COORDINATOR_API, req)
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
        // JoinGroup may return MEMBER_ID_REQUIRED once; retry with assigned id.
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
            range_assign(
                &self.topic,
                &topic_parts,
                &join.members,
            )?
        } else {
            Vec::new()
        };
        let sync = self
            .sync_group(
                join.member_id.as_str(),
                join.generation_id,
                assignments,
            )
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
            // Solo member / broker skipped assignment bytes — keep metadata partitions.
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
        req.session_timeout_ms = 30_000;
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

    async fn connect_coordinator(&self) -> Result<TcpStream, KafkaWireError> {
        if let Some(addr) = self.coordinator.lock().clone() {
            match timeout(Duration::from_secs(3), TcpStream::connect(addr.as_str())).await {
                Ok(Ok(s)) => return Ok(s),
                Ok(Err(_)) | Err(_) => {}
            }
        }
        self.connect_first().await
    }

    async fn connect_first(&self) -> Result<TcpStream, KafkaWireError> {
        if self.brokers.is_empty() {
            return Err(KafkaWireError::NoBrokers);
        }
        let mut last = None;
        for b in &self.brokers {
            match timeout(Duration::from_secs(3), TcpStream::connect(b.as_str())).await {
                Ok(Ok(s)) => return Ok(s),
                Ok(Err(e)) => last = Some(KafkaWireError::Io(e)),
                Err(_) => last = Some(KafkaWireError::Timeout),
            }
        }
        Err(last.unwrap_or(KafkaWireError::NoBrokers))
    }

    async fn roundtrip<Req, Resp>(
        &self,
        stream: &mut TcpStream,
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
}

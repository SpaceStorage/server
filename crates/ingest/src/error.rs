//! Ingest validation / error codes (009).

use thiserror::Error;

pub const UI_INGEST_SLICE11_REQUIRED: &str = "UiIngestSlice11Required";

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum IngestError {
    #[error("ingest_missing_target")]
    MissingTarget,
    #[error("ingest_unknown_type")]
    UnknownType,
    #[error("ingest_forbidden")]
    Forbidden,
    #[error("ingest_write_only")]
    WriteOnly,
    #[error("ingest_kafka_no_brokers")]
    KafkaNoBrokers,
    #[error("ingest_kafka_no_topic")]
    KafkaNoTopic,
    #[error("ingest_kafka_no_group")]
    KafkaNoGroup,
    #[error("ingest_cert_inline")]
    CertInline,
    #[error("ingest_syslog_cluster_only")]
    SyslogClusterOnly,
    #[error("{UI_INGEST_SLICE11_REQUIRED}")]
    Slice11Required,
    #[error("not_found")]
    NotFound,
    #[error("{0}")]
    Invalid(String),
}

impl IngestError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::MissingTarget => "ingest_missing_target",
            Self::UnknownType => "ingest_unknown_type",
            Self::Forbidden => "ingest_forbidden",
            Self::WriteOnly => "ingest_write_only",
            Self::KafkaNoBrokers => "ingest_kafka_no_brokers",
            Self::KafkaNoTopic => "ingest_kafka_no_topic",
            Self::KafkaNoGroup => "ingest_kafka_no_group",
            Self::CertInline => "ingest_cert_inline",
            Self::SyslogClusterOnly => "ingest_syslog_cluster_only",
            Self::Slice11Required => UI_INGEST_SLICE11_REQUIRED,
            Self::NotFound => "not_found",
            Self::Invalid(_) => "invalid",
        }
    }
}

//! Kafka payload mapping: raw UTF-8 (default) or JSON object fields.

use crate::record::LogRecord;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PayloadFormat {
    #[default]
    Raw,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    InvalidUtf8,
    JsonNotObject,
}

impl FormatError {
    pub fn reason(&self) -> &'static str {
        match self {
            Self::InvalidUtf8 => "utf8",
            Self::JsonNotObject => "json_root",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct KafkaSidecar {
    pub key: Option<String>,
    pub topic: String,
    pub partition: i32,
    pub offset: i64,
    pub broker_ts: Option<String>,
}

pub fn map_kafka_payload(
    format: PayloadFormat,
    value: &[u8],
    sidecar: &KafkaSidecar,
) -> Result<LogRecord, FormatError> {
    match format {
        PayloadFormat::Raw => {
            let message = std::str::from_utf8(value)
                .map_err(|_| FormatError::InvalidUtf8)?
                .to_string();
            Ok(LogRecord {
                timestamp: sidecar.broker_ts.clone(),
                message,
                kafka_key: sidecar.key.clone(),
                kafka_topic: Some(sidecar.topic.clone()),
                kafka_partition: Some(sidecar.partition),
                kafka_offset: Some(sidecar.offset),
                attrs: Value::Null,
                ..Default::default()
            })
        }
        PayloadFormat::Json => {
            let v: Value =
                serde_json::from_slice(value).map_err(|_| FormatError::JsonNotObject)?;
            let obj = v.as_object().ok_or(FormatError::JsonNotObject)?;
            let mut rec = LogRecord {
                timestamp: sidecar
                    .broker_ts
                    .clone()
                    .or_else(|| obj.get("timestamp").and_then(|x| x.as_str()).map(str::to_string)),
                message: obj
                    .get("message")
                    .map(|m| match m {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    })
                    .unwrap_or_default(),
                kafka_key: sidecar.key.clone(),
                kafka_topic: Some(sidecar.topic.clone()),
                kafka_partition: Some(sidecar.partition),
                kafka_offset: Some(sidecar.offset),
                severity: obj
                    .get("severity")
                    .and_then(|x| x.as_str())
                    .map(str::to_string),
                host: obj.get("host").and_then(|x| x.as_str()).map(str::to_string),
                app_name: obj
                    .get("app_name")
                    .and_then(|x| x.as_str())
                    .map(str::to_string),
                procid: obj
                    .get("procid")
                    .and_then(|x| x.as_str())
                    .map(str::to_string),
                msgid: obj
                    .get("msgid")
                    .and_then(|x| x.as_str())
                    .map(str::to_string),
                facility: obj
                    .get("facility")
                    .and_then(|x| x.as_str())
                    .map(str::to_string),
                attrs: Value::Null,
            };
            let known = [
                "timestamp",
                "message",
                "severity",
                "host",
                "app_name",
                "procid",
                "msgid",
                "facility",
            ];
            let mut attrs = serde_json::Map::new();
            for (k, v) in obj {
                if !known.contains(&k.as_str()) {
                    attrs.insert(k.clone(), v.clone());
                }
            }
            if !attrs.is_empty() {
                rec.attrs = Value::Object(attrs);
            }
            Ok(rec)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_invalid_utf8() {
        let err = map_kafka_payload(
            PayloadFormat::Raw,
            &[0xff, 0xfe],
            &KafkaSidecar {
                topic: "t".into(),
                partition: 0,
                offset: 1,
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(err, FormatError::InvalidUtf8);
    }

    #[test]
    fn raw_success_sidecars() {
        let rec = map_kafka_payload(
            PayloadFormat::Raw,
            b"hello",
            &KafkaSidecar {
                key: Some("k".into()),
                topic: "logs".into(),
                partition: 2,
                offset: 9,
                broker_ts: Some("t0".into()),
            },
        )
        .unwrap();
        assert_eq!(rec.message, "hello");
        assert_eq!(rec.kafka_topic.as_deref(), Some("logs"));
        assert_eq!(rec.kafka_partition, Some(2));
        assert_eq!(rec.kafka_offset, Some(9));
    }

    #[test]
    fn json_non_object() {
        let err = map_kafka_payload(
            PayloadFormat::Json,
            b"[1,2]",
            &KafkaSidecar {
                topic: "t".into(),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(err, FormatError::JsonNotObject);
    }

    #[test]
    fn json_maps_fields_and_attrs() {
        let rec = map_kafka_payload(
            PayloadFormat::Json,
            br#"{"message":"hi","extra":1}"#,
            &KafkaSidecar {
                topic: "t".into(),
                partition: 0,
                offset: 0,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(rec.message, "hi");
        assert_eq!(rec.attrs.get("extra").and_then(|v| v.as_i64()), Some(1));
    }
}

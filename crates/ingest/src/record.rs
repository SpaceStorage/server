//! Stored log record shape + append helper.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct LogRecord {
    pub timestamp: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kafka_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kafka_topic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kafka_partition: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kafka_offset: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub procid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub msgid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub facility: Option<String>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub attrs: Value,
}

impl LogRecord {
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| json!({}))
    }
}

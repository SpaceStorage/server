//! Wire / handshake version constants (contracts/wire-versions.md).

use serde::{Deserialize, Serialize};

/// Protocol identity for classify and wire constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProtocolId {
    PostgreSql,
    Cassandra,
    Redis,
    Elasticsearch,
    ClickHouse,
    ClickHouseHttp,
    S3,
    WebDav,
}

impl ProtocolId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PostgreSql => "postgresql",
            Self::Cassandra => "cassandra",
            Self::Redis => "redis",
            Self::Elasticsearch => "elasticsearch",
            Self::ClickHouse => "clickhouse",
            Self::ClickHouseHttp => "clickhouse-http",
            Self::S3 => "s3",
            Self::WebDav => "webdav",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "postgresql" | "postgres" | "pg" => Some(Self::PostgreSql),
            "cassandra" => Some(Self::Cassandra),
            "redis" => Some(Self::Redis),
            "elasticsearch" | "es" => Some(Self::Elasticsearch),
            "clickhouse" => Some(Self::ClickHouse),
            "clickhouse-http" => Some(Self::ClickHouseHttp),
            "s3" => Some(Self::S3),
            "webdav" => Some(Self::WebDav),
            _ => None,
        }
    }
}

/// Declared wire / handshake label for a protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WireVersion {
    pub protocol: ProtocolId,
    pub label: &'static str,
}

pub const PG_WIRE_3_0: WireVersion = WireVersion {
    protocol: ProtocolId::PostgreSql,
    label: "3.0",
};

pub const CASSANDRA_V4: WireVersion = WireVersion {
    protocol: ProtocolId::Cassandra,
    label: "v4",
};

pub const CASSANDRA_V5: WireVersion = WireVersion {
    protocol: ProtocolId::Cassandra,
    label: "v5",
};

pub const REDIS_RESP2: WireVersion = WireVersion {
    protocol: ProtocolId::Redis,
    label: "RESP2",
};

pub const ELASTICSEARCH_HTTP11: WireVersion = WireVersion {
    protocol: ProtocolId::Elasticsearch,
    label: "HTTP/1.1",
};

pub const CLICKHOUSE_NATIVE: WireVersion = WireVersion {
    protocol: ProtocolId::ClickHouse,
    label: "native",
};

pub const CLICKHOUSE_HTTP: WireVersion = WireVersion {
    protocol: ProtocolId::ClickHouseHttp,
    label: "http",
};

pub const S3_SIGV4: WireVersion = WireVersion {
    protocol: ProtocolId::S3,
    label: "SigV4",
};

pub const WEBDAV_RFC4918: WireVersion = WireVersion {
    protocol: ProtocolId::WebDav,
    label: "RFC4918",
};

/// RESP3 is absent from all current dialect profiles.
pub const RESP3_IN_PROFILE: bool = false;

/// Primary wire label(s) for a protocol in current profiles.
pub fn wire_versions(protocol: ProtocolId) -> &'static [WireVersion] {
    match protocol {
        ProtocolId::PostgreSql => &[PG_WIRE_3_0],
        ProtocolId::Cassandra => &[CASSANDRA_V4, CASSANDRA_V5],
        ProtocolId::Redis => &[REDIS_RESP2],
        ProtocolId::Elasticsearch => &[ELASTICSEARCH_HTTP11],
        ProtocolId::ClickHouse => &[CLICKHOUSE_NATIVE],
        ProtocolId::ClickHouseHttp => &[CLICKHOUSE_HTTP],
        ProtocolId::S3 => &[S3_SIGV4],
        ProtocolId::WebDav => &[WEBDAV_RFC4918],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redis_is_resp2_not_resp3() {
        assert_eq!(wire_versions(ProtocolId::Redis), &[REDIS_RESP2]);
        assert!(!RESP3_IN_PROFILE);
    }

    #[test]
    fn cassandra_v4_and_v5() {
        let v = wire_versions(ProtocolId::Cassandra);
        assert!(v.iter().any(|w| w.label == "v4"));
        assert!(v.iter().any(|w| w.label == "v5"));
    }
}

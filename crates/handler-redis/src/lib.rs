//! Redis first-binary dialect (002) — K/V MUST list; off-list errors.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedisReply {
    Ok,
    Bulk(Option<Vec<u8>>),
    Integer(i64),
    Error(String),
}

const MUST: &[&str] = &[
    "AUTH", "PING", "GET", "SET", "DEL", "EXISTS", "SCAN", "SELECT", "TTL", "EXPIRE", "PTTL",
];

pub fn dispatch(cmd: &str, _args: &[&str]) -> RedisReply {
    let c = cmd.to_ascii_uppercase();
    if MUST.iter().any(|m| *m == c) {
        match c.as_str() {
            "PING" => RedisReply::Bulk(Some(b"PONG".to_vec())),
            "SELECT" => RedisReply::Ok, // no-op
            "AUTH" => RedisReply::Ok,
            _ => RedisReply::Bulk(None),
        }
    } else {
        RedisReply::Error(format!("ERR unknown command '{cmd}'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hget_errors() {
        assert!(matches!(dispatch("HGET", &["k", "f"]), RedisReply::Error(_)));
        assert!(matches!(dispatch("JSON.GET", &["k"]), RedisReply::Error(_)));
        assert!(matches!(dispatch("PING", &[]), RedisReply::Bulk(Some(_))));
    }
}

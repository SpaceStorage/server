//! RFC 5424 syslog parser (preferred).

use crate::record::LogRecord;
use serde_json::{json, Map, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError;

/// Parse RFC 5424 datagram or octet-counted message body (without length prefix).
pub fn parse(input: &str) -> Result<LogRecord, ParseError> {
    let s = input.trim_start();
    if !s.starts_with('<') {
        return Err(ParseError);
    }
    let pri_end = s.find('>').ok_or(ParseError)?;
    let pri: u32 = s[1..pri_end].parse().map_err(|_| ParseError)?;
    let facility = pri >> 3;
    let severity = pri & 0x07;
    let rest = &s[pri_end + 1..];
    // VERSION SP TIMESTAMP SP HOSTNAME SP APP-NAME SP PROCID SP MSGID SP STRUCTURED-DATA [SP MSG]
    let mut parts = rest.splitn(7, ' ');
    let version = parts.next().ok_or(ParseError)?;
    if version != "1" {
        return Err(ParseError);
    }
    let timestamp = parts.next().ok_or(ParseError)?;
    let host = parts.next().ok_or(ParseError)?;
    let app = parts.next().ok_or(ParseError)?;
    let procid = parts.next().ok_or(ParseError)?;
    let msgid = parts.next().ok_or(ParseError)?;
    let rem = parts.next().unwrap_or("");
    let (sd, msg) = if rem.starts_with('[') {
        // structured data until " ] " or lone "-"
        if let Some(idx) = rem.find("] ") {
            let sd = &rem[..=idx];
            let msg = rem[idx + 2..].to_string();
            (Some(sd.to_string()), msg)
        } else if rem == "-" || rem.starts_with("- ") {
            let msg = rem.strip_prefix("- ").unwrap_or("").to_string();
            (None, msg)
        } else {
            (Some(rem.to_string()), String::new())
        }
    } else if rem == "-" {
        (None, String::new())
    } else if let Some(stripped) = rem.strip_prefix("- ") {
        (None, stripped.to_string())
    } else {
        (None, rem.to_string())
    };
    let mut attrs = Value::Null;
    if let Some(sd) = sd {
        let mut m = Map::new();
        m.insert("structured_data".into(), json!(sd));
        attrs = Value::Object(m);
    }
    Ok(LogRecord {
        timestamp: Some(timestamp.to_string()),
        message: msg,
        host: Some(host.to_string()),
        app_name: Some(app.to_string()),
        procid: Some(procid.to_string()),
        msgid: Some(msgid.to_string()),
        facility: Some(facility.to_string()),
        severity: Some(severity.to_string()),
        attrs,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_5424() {
        let line = r#"<34>1 2024-01-01T00:00:00Z host app 123 msgid - hello world"#;
        let rec = parse(line).unwrap();
        assert_eq!(rec.message, "hello world");
        assert_eq!(rec.host.as_deref(), Some("host"));
        assert_eq!(rec.app_name.as_deref(), Some("app"));
        assert_eq!(rec.facility.as_deref(), Some("4"));
        assert_eq!(rec.severity.as_deref(), Some("2"));
    }

    #[test]
    fn malformed_errors() {
        assert!(parse("not-syslog").is_err());
        assert!(parse("<x>1 a b c d e f").is_err());
    }
}

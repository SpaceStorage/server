//! RFC 3164 syslog parser (accepted fallback).

use crate::record::LogRecord;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError;

/// Parse BSD syslog: `<PRI>TIMESTAMP HOSTNAME TAG: MESSAGE` (simplified).
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
    // TIMESTAMP is "Mmm dd hh:mm:ss" (15 chars with possible space padding on day)
    if rest.len() < 16 {
        return Err(ParseError);
    }
    let timestamp = rest[..15].trim().to_string();
    let after = rest[15..].trim_start();
    let mut it = after.splitn(2, ' ');
    let host = it.next().ok_or(ParseError)?.to_string();
    let rem = it.next().unwrap_or("").trim_start();
    let (tag, msg) = if let Some(idx) = rem.find(':') {
        let tag = rem[..idx].trim().to_string();
        let msg = rem[idx + 1..].trim_start().to_string();
        (tag, msg)
    } else {
        (String::new(), rem.to_string())
    };
    if host.is_empty() {
        return Err(ParseError);
    }
    Ok(LogRecord {
        timestamp: Some(timestamp),
        message: msg,
        host: Some(host),
        app_name: if tag.is_empty() { None } else { Some(tag) },
        facility: Some(facility.to_string()),
        severity: Some(severity.to_string()),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_3164() {
        let line = "<34>Oct 11 22:14:15 mymachine su: 'su root' failed";
        let rec = parse(line).unwrap();
        assert_eq!(rec.host.as_deref(), Some("mymachine"));
        assert_eq!(rec.app_name.as_deref(), Some("su"));
        assert!(rec.message.contains("failed"));
    }

    #[test]
    fn malformed_errors() {
        assert!(parse("garbage").is_err());
    }
}

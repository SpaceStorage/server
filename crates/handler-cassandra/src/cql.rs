//! Minimal CQL keyword scan → dispatch verb + args.

/// Map a CQL statement to `(verb, args)` for [`crate::dispatch::dispatch`].
pub fn classify_cql(cql: &str) -> Option<(String, Vec<String>)> {
    let trimmed = cql.trim();
    let upper = trimmed.to_ascii_uppercase();
    if upper.is_empty() {
        return None;
    }
    if upper.starts_with("CREATE TABLE") {
        let table = ident_after(trimmed, "CREATE TABLE")?;
        return Some(("CREATE".into(), vec!["TABLE".into(), table]));
    }
    if upper.starts_with("CREATE KEYSPACE") {
        let ks = ident_after(trimmed, "CREATE KEYSPACE")?;
        return Some(("CREATE".into(), vec!["KEYSPACE".into(), ks]));
    }
    if upper.starts_with("DROP TABLE") {
        let table = ident_after(trimmed, "DROP TABLE")?;
        return Some(("DROP".into(), vec!["TABLE".into(), table]));
    }
    if upper.starts_with("INSERT INTO") {
        let rest = trimmed[12..].trim_start();
        let table = first_ident(rest)?;
        let key = extract_quoted_or_word(cql, "VALUES")?.0;
        let val = extract_second_value(cql).unwrap_or_default();
        return Some((
            "INSERT".into(),
            vec![table, key, val],
        ));
    }
    if upper.starts_with("SELECT") {
        let table = find_from_table(trimmed, &upper)?;
        if let Some(id) = extract_where_id(cql) {
            return Some(("SELECT".into(), vec![table, id]));
        }
        return Some(("SELECT".into(), vec![table]));
    }
    if upper.starts_with("USE ") {
        let ks = ident_after(trimmed, "USE")?;
        return Some(("USE".into(), vec![ks]));
    }
    None
}

fn ident_after(original: &str, prefix: &str) -> Option<String> {
    let upper = original.to_ascii_uppercase();
    let start = upper.find(prefix)? + prefix.len();
    first_ident(original[start..].trim_start())
}

fn first_ident(s: &str) -> Option<String> {
    let s = s.trim_start();
    if s.is_empty() {
        return None;
    }
    if s.starts_with('"') {
        let end = s[1..].find('"')? + 1;
        return Some(s[1..end].to_string());
    }
    let end = s
        .find(|c: char| c.is_whitespace() || c == '(' || c == ';')
        .unwrap_or(s.len());
    Some(s[..end].trim_matches('"').to_string())
}

fn find_from_table(original: &str, upper: &str) -> Option<String> {
    let idx = upper.find(" FROM ")?;
    let rest = &original[idx + 6..];
    first_ident(rest)
}

fn extract_where_id(cql: &str) -> Option<String> {
    let upper = cql.to_ascii_uppercase();
    let idx = upper.find(" WHERE ")?;
    let rest = &cql[idx + 7..];
    let rest_upper = &upper[idx + 7..];
    let id_pos = rest_upper.find("ID")?;
    let after = &rest[id_pos + 2..];
    let after = after.trim_start();
    let after = after.strip_prefix('=')?.trim_start();
    parse_literal(after)
}

fn extract_quoted_or_word(cql: &str, keyword: &str) -> Option<(String, String)> {
    let upper = cql.to_ascii_uppercase();
    let idx = upper.find(keyword)?;
    let rest = &cql[idx + keyword.len()..];
    let rest = rest.trim_start();
    if rest.starts_with('(') {
        let inner = rest.trim_start_matches('(').trim();
        let first = inner.split(',').next()?.trim();
        return Some((parse_literal(first)?, String::new()));
    }
    Some((parse_literal(rest)?, String::new()))
}

fn extract_second_value(cql: &str) -> Option<String> {
    let upper = cql.to_ascii_uppercase();
    let idx = upper.find("VALUES")?;
    let rest = &cql[idx + 6..];
    let rest = rest.trim_start().trim_start_matches('(').trim();
    let mut parts = split_values(rest);
    parts.next();
    parts.next().map(|s| parse_literal(s.trim()).unwrap_or_default())
}

fn split_values(s: &str) -> impl Iterator<Item = &str> {
    s.split(',').map(|p| p.trim().trim_end_matches(')').trim())
}

fn parse_literal(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if (s.starts_with('\'') && s.ends_with('\'')) || (s.starts_with('"') && s.ends_with('"')) {
        return Some(s[1..s.len() - 1].to_string());
    }
    let end = s
        .find(|c: char| c.is_whitespace() || c == ',' || c == ')' || c == ';')
        .unwrap_or(s.len());
    Some(s[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_ddl() {
        let (v, a) = classify_cql("CREATE TABLE t (id text PRIMARY KEY)").unwrap();
        assert_eq!(v, "CREATE");
        assert_eq!(a[1], "t");
        let (v, _) = classify_cql("DROP TABLE t").unwrap();
        assert_eq!(v, "DROP");
    }

    #[test]
    fn maps_dml() {
        let (v, a) = classify_cql("INSERT INTO t (id, v) VALUES ('1', 'a')").unwrap();
        assert_eq!(v, "INSERT");
        assert_eq!(a[0], "t");
        assert_eq!(a[1], "1");
        let (v, a) = classify_cql("SELECT v FROM t WHERE id='1'").unwrap();
        assert_eq!(v, "SELECT");
        assert_eq!(a[1], "1");
    }
}

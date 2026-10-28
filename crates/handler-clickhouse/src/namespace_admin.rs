//! `CREATE DATABASE` / `RENAME DATABASE` → cluster `append_tenancy`.

use spacestorage_tenancy::TenancyError;

use crate::dispatch::{ClickHouseReply, SessionState};

fn admin_only(login: &str) -> Result<(), ClickHouseReply> {
    if login == "admin" {
        Ok(())
    } else {
        Err(ClickHouseReply::Error {
            code: "497".into(),
            message: TenancyError::NotClusterAdmin.to_string(),
        })
    }
}

fn parse_create_database(sql: &str) -> Option<String> {
    let upper = sql.to_ascii_uppercase();
    if !upper.starts_with("CREATE DATABASE") {
        return None;
    }
    let parts: Vec<&str> = sql.split_whitespace().collect();
    let db_idx = parts
        .iter()
        .position(|p| p.eq_ignore_ascii_case("DATABASE"))?;
    let mut i = db_idx + 1;
    while i < parts.len() {
        let token = parts[i];
        if token.eq_ignore_ascii_case("IF")
            || token.eq_ignore_ascii_case("NOT")
            || token.eq_ignore_ascii_case("EXISTS")
        {
            i += 1;
            continue;
        }
        let name = token
            .trim_matches(|c| c == '`' || c == ';')
            .trim_matches('"');
        if !name.is_empty() {
            return Some(name.to_string());
        }
        break;
    }
    None
}

fn parse_rename_database(sql: &str) -> Option<(String, String)> {
    let upper = sql.to_ascii_uppercase();
    if !upper.starts_with("RENAME DATABASE") {
        return None;
    }
    let parts: Vec<&str> = sql.split_whitespace().collect();
    let db_idx = parts
        .iter()
        .position(|p| p.eq_ignore_ascii_case("DATABASE"))?;
    let old = parts.get(db_idx + 1)?.trim_matches(|c| c == '`' || c == ';');
    let to_idx = parts.iter().position(|p| p.eq_ignore_ascii_case("TO"))?;
    let new = parts.get(to_idx + 1)?.trim_matches(|c| c == '`' || c == ';');
    Some((old.to_string(), new.to_string()))
}

/// Handle namespace DDL when `namespace_control` is wired; `None` if SQL is not admin DDL.
pub async fn try_namespace_admin_sql(
    session: &mut SessionState,
    sql: &str,
) -> Option<ClickHouseReply> {
    let login = session.principal.as_deref().unwrap_or("");
    if let Some(name) = parse_create_database(sql) {
        if let Err(reply) = admin_only(login) {
            return Some(reply);
        }
        let Some(control) = session.namespace_control.as_ref() else {
            if session.tenancy.is_some() {
                return Some(ClickHouseReply::Error {
                    code: "server_error".into(),
                    message: "controlplane not started".into(),
                });
            }
            session.namespace = name;
            return Some(ClickHouseReply::Ok);
        };
        return Some(match control.create(&name).await {
            Ok(()) => {
                session.namespace = name;
                ClickHouseReply::Ok
            }
            Err(message) => ClickHouseReply::Error {
                code: "server_error".into(),
                message,
            },
        });
    }
    if let Some((old, new)) = parse_rename_database(sql) {
        if let Err(reply) = admin_only(login) {
            return Some(reply);
        }
        let Some(control) = session.namespace_control.as_ref() else {
            if session.tenancy.is_some() {
                return Some(ClickHouseReply::Error {
                    code: "server_error".into(),
                    message: "controlplane not started".into(),
                });
            }
            return Some(ClickHouseReply::Ok);
        };
        return Some(match control.rename(&old, &new).await {
            Ok(()) => ClickHouseReply::Ok,
            Err(message) => ClickHouseReply::Error {
                code: "server_error".into(),
                message,
            },
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_create_and_rename() {
        assert_eq!(
            parse_create_database("CREATE DATABASE acme"),
            Some("acme".into())
        );
        assert_eq!(
            parse_rename_database("RENAME DATABASE acme TO contoso"),
            Some(("acme".into(), "contoso".into()))
        );
    }
}

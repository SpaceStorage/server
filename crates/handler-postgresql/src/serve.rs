//! Async PostgreSQL `Handler::serve` (Constitution II) — simple + extended.

use crate::auth::{begin_scram, finish_scram, ScramExchange, UserStore};
use crate::sql::{execute_sql, ExecResult, SharedCatalog};
use crate::wire::{
    try_read_message, try_read_startup, write_auth_ok, write_auth_sasl, write_auth_sasl_continue,
    write_auth_sasl_final, write_backend_key_data, write_bind_complete, write_command_complete,
    write_data_row, write_error, write_no_data, write_parameter_status, write_parse_complete,
    write_ready, write_row_description, write_ssl_no, FrontendMessage, FEATURE_NOT_SUPPORTED,
};
use bytes::{BufMut, BytesMut};
use spacestorage_compat::DialectProfile;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

struct PgSession {
    namespace: String,
    statements: HashMap<String, String>,
    portals: HashMap<String, String>,
}

impl Default for PgSession {
    fn default() -> Self {
        Self {
            namespace: "default".into(),
            statements: HashMap::new(),
            portals: HashMap::new(),
        }
    }
}

enum StartupAuth {
    Negotiating,
    WaitSaslInitial { namespace: String },
    WaitSaslFinal { namespace: String, exch: ScramExchange },
    Done,
}

/// Serve one PostgreSQL client connection (wire 3.0 + SCRAM-SHA-256, auto-commit dialect).
pub async fn serve<S>(
    mut stream: S,
    cancel: CancellationToken,
    catalog: SharedCatalog,
    profile: DialectProfile,
    users: Arc<UserStore>,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let mut buf = BytesMut::with_capacity(8192);
    let mut read_buf = vec![0u8; 8192];
    let mut auth = StartupAuth::Negotiating;
    let mut session = PgSession::default();

    while !matches!(auth, StartupAuth::Done) {
        tokio::select! {
            _ = cancel.cancelled() => return,
            r = stream.read(&mut read_buf) => {
                match r {
                    Ok(0) | Err(_) => return,
                    Ok(n) => {
                        buf.extend_from_slice(&read_buf[..n]);
                        loop {
                            let progressed = match &auth {
                                StartupAuth::Negotiating => {
                                    match try_read_startup(&mut buf) {
                                        Ok(None) => break,
                                        Err(e) => {
                                            let mut out = BytesMut::new();
                                            write_error(&mut out, "08P01", &e.to_string());
                                            let _ = stream.write_all(&out).await;
                                            return;
                                        }
                                        Ok(Some(msg)) => match msg {
                                            FrontendMessage::SslRequest | FrontendMessage::GssRequest => {
                                                let mut out = BytesMut::new();
                                                write_ssl_no(&mut out);
                                                if stream.write_all(&out).await.is_err() {
                                                    return;
                                                }
                                                true
                                            }
                                            FrontendMessage::CancelRequest { .. } => return,
                                            FrontendMessage::Startup { params } => {
                                                let mut ns = "default".to_string();
                                                for (k, v) in &params {
                                                    if k.eq_ignore_ascii_case("database") {
                                                        ns = v.clone();
                                                    }
                                                }
                                                let mut out = BytesMut::new();
                                                write_auth_sasl(&mut out);
                                                if stream.write_all(&out).await.is_err() {
                                                    return;
                                                }
                                                auth = StartupAuth::WaitSaslInitial { namespace: ns };
                                                true
                                            }
                                            _ => true,
                                        },
                                    }
                                }
                                StartupAuth::WaitSaslInitial { namespace } => {
                                    match try_read_message(&mut buf) {
                                        Ok(None) => break,
                                        Err(_) => return,
                                        Ok(Some(FrontendMessage::PasswordMsg(raw))) => {
                                            let Some((mech, data)) = parse_sasl_initial(&raw) else {
                                                return;
                                            };
                                            if mech != "SCRAM-SHA-256" {
                                                let mut out = BytesMut::new();
                                                write_error(&mut out, "28000", "only SCRAM-SHA-256");
                                                let _ = stream.write_all(&out).await;
                                                return;
                                            }
                                            let client_first = match std::str::from_utf8(&data) {
                                                Ok(s) => s,
                                                Err(_) => return,
                                            };
                                            match begin_scram(&users, client_first) {
                                                Ok((exch, server_first)) => {
                                                    let mut out = BytesMut::new();
                                                    write_auth_sasl_continue(&mut out, &server_first);
                                                    if stream.write_all(&out).await.is_err() {
                                                        return;
                                                    }
                                                    auth = StartupAuth::WaitSaslFinal {
                                                        namespace: namespace.clone(),
                                                        exch,
                                                    };
                                                }
                                                Err(e) => {
                                                    let mut out = BytesMut::new();
                                                    write_error(&mut out, "28P01", &e);
                                                    let _ = stream.write_all(&out).await;
                                                    return;
                                                }
                                            }
                                            true
                                        }
                                        Ok(Some(_)) => return,
                                    }
                                }
                                StartupAuth::WaitSaslFinal { .. } => {
                                    match try_read_message(&mut buf) {
                                        Ok(None) => break,
                                        Err(_) => return,
                                        Ok(Some(FrontendMessage::PasswordMsg(raw))) => {
                                            let StartupAuth::WaitSaslFinal { namespace, exch } =
                                                std::mem::replace(&mut auth, StartupAuth::Negotiating)
                                            else {
                                                return;
                                            };
                                            let client_final = match std::str::from_utf8(&raw) {
                                                Ok(s) => s,
                                                Err(_) => return,
                                            };
                                            match finish_scram(&exch, client_final) {
                                                Ok(server_final) => {
                                                    session.namespace = namespace;
                                                    let mut out = BytesMut::new();
                                                    write_auth_sasl_final(&mut out, &server_final);
                                                    write_auth_ok(&mut out);
                                                    write_parameter_status(
                                                        &mut out,
                                                        "server_version",
                                                        "16.4 (SpaceStorage)",
                                                    );
                                                    write_parameter_status(
                                                        &mut out,
                                                        "client_encoding",
                                                        "UTF8",
                                                    );
                                                    write_backend_key_data(&mut out, 42, 42);
                                                    write_ready(&mut out, b'I');
                                                    if stream.write_all(&out).await.is_err() {
                                                        return;
                                                    }
                                                    auth = StartupAuth::Done;
                                                }
                                                Err(e) => {
                                                    let mut out = BytesMut::new();
                                                    write_error(&mut out, "28P01", &e);
                                                    let _ = stream.write_all(&out).await;
                                                    return;
                                                }
                                            }
                                            true
                                        }
                                        Ok(Some(_)) => return,
                                    }
                                }
                                StartupAuth::Done => break,
                            };
                            if !progressed {
                                break;
                            }
                            if matches!(auth, StartupAuth::Done) {
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    // Message loop
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            r = stream.read(&mut read_buf) => {
                match r {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&read_buf[..n]);
                        loop {
                            match try_read_message(&mut buf) {
                                Ok(None) => break,
                                Err(e) => {
                                    let mut out = BytesMut::new();
                                    write_error(&mut out, "08P01", &e.to_string());
                                    write_ready(&mut out, b'I');
                                    let _ = stream.write_all(&out).await;
                                    return;
                                }
                                Ok(Some(msg)) => {
                                    if handle_msg(
                                        &mut stream,
                                        &catalog,
                                        profile,
                                        &mut session,
                                        msg,
                                    )
                                    .await
                                    .is_err()
                                    {
                                        return;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn parse_sasl_initial(raw: &[u8]) -> Option<(String, Vec<u8>)> {
    let end = raw.iter().position(|&b| b == 0)?;
    let mech = std::str::from_utf8(&raw[..end]).ok()?.to_string();
    if raw.len() < end + 1 + 4 {
        return None;
    }
    let len = i32::from_be_bytes(raw[end + 1..end + 5].try_into().ok()?) as usize;
    let data = raw.get(end + 5..end + 5 + len)?.to_vec();
    Some((mech, data))
}

async fn handle_msg<S>(
    stream: &mut S,
    catalog: &SharedCatalog,
    profile: DialectProfile,
    session: &mut PgSession,
    msg: FrontendMessage,
) -> Result<(), ()>
where
    S: AsyncWrite + Unpin,
{
    let mut out = BytesMut::new();
    match msg {
        FrontendMessage::Terminate => return Err(()),
        FrontendMessage::Query(sql) => {
            // Simple query may contain multiple statements separated by ;
            for stmt in split_statements(&sql) {
                render_exec(&mut out, catalog, profile, &session.namespace, &stmt);
            }
            write_ready(&mut out, b'I');
        }
        FrontendMessage::Parse { name, query, .. } => {
            session.statements.insert(name, query);
            write_parse_complete(&mut out);
        }
        FrontendMessage::Bind {
            portal,
            statement,
            params,
        } => {
            let sql = session
                .statements
                .get(&statement)
                .cloned()
                .unwrap_or_default();
            session.portals.insert(portal, bind_params(&sql, &params));
            write_bind_complete(&mut out);
        }
        FrontendMessage::Describe { typ, name } => {
            let sql = if typ == b'S' {
                session.statements.get(&name).cloned()
            } else {
                session.portals.get(&name).cloned()
            };
            match sql {
                Some(s) if first_verb(&s) == "SELECT" => {
                    // Minimal row description after a dry describe — unknown cols → NoData for DML
                    write_row_description(&mut out, &["?column?"]);
                }
                _ => write_no_data(&mut out),
            }
        }
        FrontendMessage::Execute { portal, .. } => {
            let sql = session.portals.get(&portal).cloned().unwrap_or_default();
            if !sql.is_empty() {
                render_exec(&mut out, catalog, profile, &session.namespace, &sql);
            }
        }
        FrontendMessage::Sync => {
            write_ready(&mut out, b'I');
        }
        FrontendMessage::Close { typ, name } => {
            if typ == b'S' {
                session.statements.remove(&name);
            } else {
                session.portals.remove(&name);
            }
            out.put_u8(b'3'); // CloseComplete
            out.put_i32(4);
        }
        FrontendMessage::Flush | FrontendMessage::PasswordMsg(_) | FrontendMessage::Other(_) => {}
        FrontendMessage::Startup { .. }
        | FrontendMessage::SslRequest
        | FrontendMessage::GssRequest
        | FrontendMessage::CancelRequest { .. } => {}
    }
    if stream.write_all(&out).await.is_err() {
        return Err(());
    }
    Ok(())
}

fn render_exec(
    out: &mut BytesMut,
    catalog: &SharedCatalog,
    profile: DialectProfile,
    namespace: &str,
    sql: &str,
) {
    match execute_sql(profile, catalog, namespace, sql) {
        Ok(res) => write_result(out, &res),
        Err(e) => write_error(out, &e.sqlstate, &e.message),
    }
}

fn write_result(out: &mut BytesMut, res: &ExecResult) {
    if !res.columns.is_empty() {
        let cols: Vec<&str> = res.columns.iter().map(|s| s.as_str()).collect();
        write_row_description(out, &cols);
        for row in &res.rows {
            let vals: Vec<Option<&str>> = row.iter().map(|c| c.as_deref()).collect();
            write_data_row(out, &vals);
        }
    }
    write_command_complete(out, &res.tag);
}

fn split_statements(sql: &str) -> Vec<String> {
    sql.split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn first_verb(sql: &str) -> String {
    sql.trim_start()
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_uppercase()
}

fn bind_params(sql: &str, params: &[Option<Vec<u8>>]) -> String {
    let mut out = sql.to_string();
    for (i, p) in params.iter().enumerate() {
        let ph = format!("${}", i + 1);
        let lit = match p {
            None => "NULL".to_string(),
            Some(b) => match std::str::from_utf8(b) {
                Ok(s) => format!("'{}'", s.replace('\'', "''")),
                Err(_) => format!("'{}'", String::from_utf8_lossy(b).replace('\'', "''")),
            },
        };
        out = out.replacen(&ph, &lit, 1);
    }
    out
}

/// Sync helper for unit tests — prefer [`serve`].
pub fn exec_sql_sync(
    catalog: &SharedCatalog,
    namespace: &str,
    sql: &str,
) -> Result<(), (String, String)> {
    execute_sql(DialectProfile::FirstBinary, catalog, namespace, sql).map(|_| ()).map_err(|e| {
        (e.sqlstate, e.message)
    })
}

#[allow(dead_code)]
fn _feature_const() -> &'static str {
    FEATURE_NOT_SUPPORTED
}

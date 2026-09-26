//! Async CQL native v4/v5 serve loop (HandlersComplete smoke).

use std::sync::{Arc, RwLock};

use bytes::BytesMut;
use spacestorage_compat::DialectProfile;
use spacestorage_protocol_core::{default_timeout, peek_signature, ProtocolFamily};
use spacestorage_types::ContainerCatalog;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use tracing::debug;

use crate::cql::classify_cql;
use crate::dispatch::{CassandraReply, SessionState, dispatch};
use crate::frame::{
    encode_auth_success, encode_authenticate, encode_error, encode_protocol_error,
    encode_ready, encode_rows_result, encode_supported, encode_void_result, parse_options,
    parse_query_cql, parse_startup, read_bytes, try_decode_frame, OP_AUTH_RESPONSE,
    OP_OPTIONS, OP_QUERY, OP_STARTUP, TYPE_VARCHAR, VERSION_V4,
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConnPhase {
    PreAuth,
    Ready,
}

pub struct CassandraHandler {
    pub catalog: Arc<RwLock<ContainerCatalog>>,
    pub profile: DialectProfile,
    pub namespace: String,
    pub expected_user: String,
    pub expected_pass: String,
}

impl CassandraHandler {
    pub fn with_demo(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self {
            catalog,
            profile: DialectProfile::HandlersComplete,
            namespace: "demo".into(),
            expected_user: "demo".into(),
            expected_pass: "demo".into(),
        }
    }

    pub async fn serve<S>(&self, stream: S, cancel: CancellationToken)
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        let (mut reader, mut writer) = tokio::io::split(stream);
        let mut buf = match peek_signature(&mut reader, ProtocolFamily::Cassandra, default_timeout())
            .await
        {
            Ok(peeked) => BytesMut::from(peeked.as_slice()),
            Err((_err, _refusal)) => {
                let frame = encode_protocol_error(VERSION_V4, 0, "protocol mismatch");
                let _ = writer.write_all(&frame).await;
                return;
            }
        };

        let mut session = SessionState {
            catalog: Arc::clone(&self.catalog),
            namespace: self.namespace.clone(),
            profile: self.profile,
        };
        let mut phase = ConnPhase::PreAuth;
        let mut wire_version = VERSION_V4;
        let mut read_buf = vec![0u8; 4096];

        loop {
            if cancel.is_cancelled() {
                break;
            }
            if !drain_frames(
                &mut buf,
                &mut writer,
                &mut session,
                &mut phase,
                &mut wire_version,
                &self.expected_user,
                &self.expected_pass,
            )
            .await
            {
                return;
            }

            tokio::select! {
                _ = cancel.cancelled() => break,
                n = reader.read(&mut read_buf) => {
                    match n {
                        Ok(0) | Err(_) => break,
                        Ok(n) => buf.extend_from_slice(&read_buf[..n]),
                    }
                }
            }
        }
    }
}

async fn drain_frames<W: AsyncWrite + Unpin>(
    buf: &mut BytesMut,
    writer: &mut W,
    session: &mut SessionState,
    phase: &mut ConnPhase,
    wire_version: &mut u8,
    expected_user: &str,
    expected_pass: &str,
) -> bool {
    loop {
        match try_decode_frame(buf) {
            Ok(None) => return true,
            Err(e) => {
                debug!(error = %e, "cql frame decode");
                let frame = encode_protocol_error(*wire_version, 0, &e.to_string());
                let _ = writer.write_all(&frame).await;
                return false;
            }
            Ok(Some(frame)) => {
                *wire_version = frame.version & 0x7f;
                let replies = match handle_frame(
                    session,
                    phase,
                    &frame,
                    expected_user,
                    expected_pass,
                ) {
                    Ok(r) => r,
                    Err(msg) => {
                        vec![encode_error(
                            *wire_version,
                            frame.stream,
                            crate::frame::ERR_PROTOCOL,
                            &msg,
                        )]
                    }
                };
                for reply in replies {
                    if writer.write_all(&reply).await.is_err() {
                        return false;
                    }
                }
                if writer.flush().await.is_err() {
                    return false;
                }
            }
        }
    }
}

fn handle_frame(
    session: &mut SessionState,
    phase: &mut ConnPhase,
    frame: &crate::frame::Frame,
    expected_user: &str,
    expected_pass: &str,
) -> Result<Vec<Vec<u8>>, String> {
    let v = frame.version;
    let stream = frame.stream;
    match frame.opcode {
        OP_OPTIONS => {
            parse_options(&frame.body).map_err(|e| e.to_string())?;
            Ok(vec![encode_supported(v, stream)])
        }
        OP_STARTUP => {
            let _opts = parse_startup(&frame.body).map_err(|e| e.to_string())?;
            Ok(vec![encode_authenticate(v, stream)])
        }
        OP_AUTH_RESPONSE if *phase == ConnPhase::PreAuth => {
            let mut off = 0;
            let token = read_bytes(&frame.body, &mut off).map_err(|e| e.to_string())?;
            let token = token.ok_or_else(|| "missing auth token".to_string())?;
            if !check_sasl_plain(&token, expected_user, expected_pass) {
                return Ok(vec![encode_error(
                    v,
                    stream,
                    0x0100, // AUTHENTICATION_ERROR
                    "Invalid credentials",
                )]);
            }
            *phase = ConnPhase::Ready;
            Ok(vec![
                encode_auth_success(v, stream),
                encode_ready(v, stream),
            ])
        }
        OP_QUERY if *phase == ConnPhase::Ready => {
            let cql = parse_query_cql(&frame.body).map_err(|e| e.to_string())?;
            let reply = if let Some((verb, args)) = classify_cql(&cql) {
                let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
                dispatch(session, &verb, &arg_refs)
            } else {
                CassandraReply::Error {
                    code: "invalid".into(),
                    message: format!("unsupported CQL: {cql}"),
                }
            };
            Ok(vec![reply_to_frame(v, stream, &reply)])
        }
        OP_QUERY => Err("connection not ready".into()),
        _ => Err(format!("unsupported opcode 0x{:02x}", frame.opcode)),
    }
}

fn check_sasl_plain(token: &[u8], user: &str, pass: &str) -> bool {
    let s = match std::str::from_utf8(token) {
        Ok(x) => x,
        Err(_) => return false,
    };
    let parts: Vec<&str> = s.split('\0').collect();
    if parts.len() < 3 {
        return false;
    }
    parts[1] == user && parts[2] == pass
}

fn reply_to_frame(version: u8, stream: i16, reply: &CassandraReply) -> Vec<u8> {
    match reply {
        CassandraReply::Ok | CassandraReply::Prepared(_) => encode_void_result(version, stream),
        CassandraReply::Rows(rows) => {
            let data: Vec<Vec<(&str, &str)>> = rows
                .iter()
                .map(|(k, v)| vec![("id", k.as_str()), ("v", v.as_str())])
                .collect();
            encode_rows_result(
                version,
                stream,
                &[("id", TYPE_VARCHAR), ("v", TYPE_VARCHAR)],
                &data,
            )
        }
        CassandraReply::Error { message, .. } => {
            encode_error(version, stream, crate::frame::ERR_PROTOCOL, message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{
        encode_frame, write_bytes, write_long_string, write_string_map, OP_QUERY, OP_RESULT,
        OP_STARTUP, VERSION_V4,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn startup_body() -> Vec<u8> {
        let mut body = Vec::new();
        write_string_map(
            &mut body,
            &[("CQL_VERSION", "3.0.0"), ("DRIVER_NAME", "test")],
        );
        body
    }

    fn auth_plain(user: &str, pass: &str) -> Vec<u8> {
        let mut token = Vec::new();
        token.push(0);
        token.extend_from_slice(user.as_bytes());
        token.push(0);
        token.extend_from_slice(pass.as_bytes());
        let mut body = Vec::new();
        write_bytes(&mut body, &token);
        body
    }

    fn query_body(cql: &str) -> Vec<u8> {
        let mut body = Vec::new();
        write_long_string(&mut body, cql);
        body.extend_from_slice(&0x0001u16.to_be_bytes()); // ONE
        body
    }

    async fn read_one_frame(
        cr: &mut tokio::io::ReadHalf<tokio::io::DuplexStream>,
        acc: &mut BytesMut,
    ) -> crate::frame::Frame {
        loop {
            if let Ok(Some(f)) = try_decode_frame(acc) {
                return f;
            }
            let mut chunk = [0u8; 4096];
            let n = cr.read(&mut chunk).await.expect("read");
            assert!(n > 0, "unexpected eof");
            acc.extend_from_slice(&chunk[..n]);
        }
    }

    #[tokio::test]
    async fn handshake_and_crud_smoke() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let handler = CassandraHandler::with_demo(Arc::clone(&cat));
        let (client, server) = tokio::io::duplex(65536);
        let (mut cr, mut cw) = tokio::io::split(client);

        // Seed first frame before serve() peek to avoid handshake-timeout races.
        cw.write_all(&encode_frame(VERSION_V4, 0, OP_OPTIONS, 0, &[]))
            .await
            .unwrap();

        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        tokio::spawn(async move {
            handler.serve(server, c2).await;
        });

        let mut acc = BytesMut::new();
        let f = read_one_frame(&mut cr, &mut acc).await;
        assert_eq!(f.opcode, crate::frame::OP_SUPPORTED);

        cw.write_all(&encode_frame(VERSION_V4, 1, OP_STARTUP, 0, &startup_body()))
            .await
            .unwrap();
        let f = read_one_frame(&mut cr, &mut acc).await;
        assert_eq!(f.opcode, crate::frame::OP_AUTHENTICATE);

        cw.write_all(&encode_frame(
            VERSION_V4,
            1,
            OP_AUTH_RESPONSE,
            0,
            &auth_plain("demo", "demo"),
        ))
        .await
        .unwrap();
        let f1 = read_one_frame(&mut cr, &mut acc).await;
        assert_eq!(f1.opcode, crate::frame::OP_AUTH_SUCCESS);
        let f2 = read_one_frame(&mut cr, &mut acc).await;
        assert_eq!(f2.opcode, crate::frame::OP_READY);

        cw.write_all(&encode_frame(
            VERSION_V4,
            2,
            OP_QUERY,
            0,
            &query_body("CREATE TABLE t (id text PRIMARY KEY)"),
        ))
        .await
        .unwrap();
        let f = read_one_frame(&mut cr, &mut acc).await;
        assert_eq!(f.opcode, OP_RESULT);

        cw.write_all(&encode_frame(
            VERSION_V4,
            3,
            OP_QUERY,
            0,
            &query_body("INSERT INTO t (id, v) VALUES ('1', 'a')"),
        ))
        .await
        .unwrap();
        cw.write_all(&encode_frame(
            VERSION_V4,
            4,
            OP_QUERY,
            0,
            &query_body("SELECT v FROM t WHERE id='1'"),
        ))
        .await
        .unwrap();
        let _void = read_one_frame(&mut cr, &mut acc).await;
        let rows = read_one_frame(&mut cr, &mut acc).await;
        assert_eq!(rows.opcode, OP_RESULT);
        let kind = i32::from_be_bytes([
            rows.body[0],
            rows.body[1],
            rows.body[2],
            rows.body[3],
        ]);
        assert_eq!(kind, crate::frame::RESULT_ROWS);

        cw.write_all(&encode_frame(
            VERSION_V4,
            5,
            OP_QUERY,
            0,
            &query_body("DROP TABLE t"),
        ))
        .await
        .unwrap();
        let _drop = read_one_frame(&mut cr, &mut acc).await;
        cancel.cancel();
    }

    #[tokio::test]
    async fn mismatch_sends_error_and_closes() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let handler = CassandraHandler::with_demo(cat);
        let (client, server) = tokio::io::duplex(4096);
        let cancel = CancellationToken::new();
        tokio::spawn(async move {
            handler.serve(server, cancel).await;
        });

        let (mut cr, mut cw) = tokio::io::split(client);
        cw.write_all(b"GET / HTTP/1.1\r\n\r\n").await.unwrap();
        cw.flush().await.unwrap();
        let mut rx = vec![0u8; 256];
        let n = cr.read(&mut rx).await.unwrap();
        assert!(n > 0);
        let mut buf = BytesMut::from(&rx[..n]);
        let f = try_decode_frame(&mut buf).unwrap().unwrap();
        assert_eq!(f.opcode, crate::frame::OP_ERROR);
    }
}

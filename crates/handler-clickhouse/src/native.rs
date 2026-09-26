//! ClickHouse native protocol serve (Hello / Query smoke).

use std::sync::{Arc, RwLock};

use bytes::BytesMut;
use spacestorage_compat::DialectProfile;
use spacestorage_protocol_core::{default_timeout, peek_signature, ProtocolFamily};
use spacestorage_types::ContainerCatalog;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::dispatch::{ClickHouseReply, SessionState};
use crate::sql::execute_sql;

pub struct ClickHouseNativeHandler {
    pub catalog: Arc<RwLock<ContainerCatalog>>,
    pub profile: DialectProfile,
    pub namespace: String,
}

impl ClickHouseNativeHandler {
    pub fn with_demo(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self {
            catalog,
            profile: DialectProfile::HandlersComplete,
            namespace: "demo".into(),
        }
    }

    pub async fn serve<S>(&self, mut stream: S, cancel: CancellationToken)
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        let peeked = match peek_signature(
            &mut stream,
            ProtocolFamily::ClickHouseNative,
            default_timeout(),
        )
        .await
        {
            Ok(buf) => buf,
            Err((_err, refusal)) => {
                let _ = stream.write_all(&refusal.body).await;
                let _ = stream.shutdown().await;
                return;
            }
        };

        let mut buf = BytesMut::from(peeked.as_slice());
        let mut session = SessionState {
            catalog: Arc::clone(&self.catalog),
            namespace: self.namespace.clone(),
            profile: self.profile,
            protocol: spacestorage_compat::ProtocolId::ClickHouse,
        };

        // Finish Hello if incomplete, then reply.
        loop {
            if consume_client_hello(&mut buf) {
                break;
            }
            let mut tmp = [0u8; 256];
            match stream.read(&mut tmp).await {
                Ok(0) | Err(_) => return,
                Ok(n) => buf.extend_from_slice(&tmp[..n]),
            }
        }
        if stream.write_all(&encode_server_hello()).await.is_err() {
            return;
        }

        let mut tmp = [0u8; 4096];
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                n = stream.read(&mut tmp) => {
                    match n {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            buf.extend_from_slice(&tmp[..n]);
                            while let Some(sql) = try_take_query(&mut buf) {
                                let reply = execute_sql(&mut session, &sql);
                                if stream.write_all(&encode_query_result(&reply)).await.is_err() {
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

pub(crate) fn write_var_uint(out: &mut Vec<u8>, mut x: u64) {
    while x >= 0x80 {
        out.push((x as u8) | 0x80);
        x >>= 7;
    }
    out.push(x as u8);
}

pub(crate) fn write_string(out: &mut Vec<u8>, s: &str) {
    write_var_uint(out, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
}

fn read_var_uint(buf: &mut BytesMut) -> Option<u64> {
    let mut x = 0u64;
    let mut s = 0u32;
    let mut i = 0;
    while i < buf.len() && i < 10 {
        let b = buf[i];
        if b < 0x80 {
            x |= (b as u64) << s;
            let _ = buf.split_to(i + 1);
            return Some(x);
        }
        x |= ((b & 0x7f) as u64) << s;
        s += 7;
        i += 1;
    }
    None
}

fn read_string(buf: &mut BytesMut) -> Option<String> {
    let snapshot = buf.clone();
    let len = match read_var_uint(buf) {
        Some(l) => l as usize,
        None => {
            *buf = snapshot;
            return None;
        }
    };
    if buf.len() < len {
        *buf = snapshot;
        return None;
    }
    let bytes = buf.split_to(len);
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Client Hello starts with packet type 0 and a length-prefixed name.
fn consume_client_hello(buf: &mut BytesMut) -> bool {
    let snapshot = buf.clone();
    let Some(packet) = read_var_uint(buf) else {
        *buf = snapshot;
        return false;
    };
    if packet != 0 {
        *buf = snapshot;
        return false;
    }
    let Some(_name) = read_string(buf) else {
        *buf = snapshot;
        return false;
    };
    // Best-effort: consume up to 4 following version varints if fully present.
    for _ in 0..4 {
        let snap = buf.clone();
        if read_var_uint(buf).is_none() {
            *buf = snap;
            break;
        }
    }
    true
}

fn encode_server_hello() -> Vec<u8> {
    let mut out = Vec::new();
    write_var_uint(&mut out, 0);
    write_string(&mut out, "SpaceStorage");
    write_var_uint(&mut out, 24);
    write_var_uint(&mut out, 8);
    write_var_uint(&mut out, 1);
    write_var_uint(&mut out, 54460);
    write_string(&mut out, "UTC");
    out
}

/// Simplified Query: packet type 1 + length-prefixed SQL string.
fn try_take_query(buf: &mut BytesMut) -> Option<String> {
    let snapshot = buf.clone();
    let packet = read_var_uint(buf)?;
    if packet != 1 {
        *buf = snapshot;
        // Drop one byte to avoid infinite stall on garbage.
        if !buf.is_empty() {
            let _ = buf.split_to(1);
        }
        return None;
    }
    match read_string(buf) {
        Some(sql) => Some(sql),
        None => {
            *buf = snapshot;
            None
        }
    }
}

fn encode_query_result(reply: &ClickHouseReply) -> Vec<u8> {
    match reply {
        ClickHouseReply::Ok => {
            let mut out = Vec::new();
            write_var_uint(&mut out, 5); // EndOfStream
            out
        }
        ClickHouseReply::Rows(rows) => {
            let mut tsv = String::new();
            for (k, v) in rows {
                tsv.push_str(k);
                tsv.push('\t');
                tsv.push_str(v);
                tsv.push('\n');
            }
            let mut out = Vec::new();
            write_var_uint(&mut out, 2); // Data
            write_string(&mut out, &tsv);
            write_var_uint(&mut out, 5);
            out
        }
        ClickHouseReply::Error { code, message } => {
            let mut out = Vec::new();
            write_var_uint(&mut out, 2);
            write_string(&mut out, &format!("Code: {code}. {message}\n"));
            write_var_uint(&mut out, 5);
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hello_query_crud_smoke() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let handler = ClickHouseNativeHandler::with_demo(cat);
        let (mut client, server) = tokio::io::duplex(8192);
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        tokio::spawn(async move { handler.serve(server, c2).await });

        let mut hello = Vec::new();
        write_var_uint(&mut hello, 0);
        write_string(&mut hello, "click");
        client.write_all(&hello).await.unwrap();

        // Read server Hello
        let mut resp = vec![0u8; 256];
        let n = tokio::time::timeout(std::time::Duration::from_millis(500), client.read(&mut resp))
            .await
            .expect("hello timeout")
            .unwrap();
        assert!(n > 0);
        assert_eq!(resp[0], 0); // server Hello type

        for sql in [
            "CREATE TABLE t (id String)",
            "INSERT INTO t VALUES ('1','x')",
            "SELECT * FROM t WHERE id = '1'",
        ] {
            let mut q = Vec::new();
            write_var_uint(&mut q, 1);
            write_string(&mut q, sql);
            client.write_all(&q).await.unwrap();
            let n = tokio::time::timeout(std::time::Duration::from_millis(500), client.read(&mut resp))
                .await
                .expect("query timeout")
                .unwrap();
            assert!(n > 0, "empty reply for {sql}");
        }
        cancel.cancel();
    }

    #[tokio::test]
    async fn mismatch_http_refused() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let handler = ClickHouseNativeHandler::with_demo(cat);
        let (mut client, server) = tokio::io::duplex(1024);
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        tokio::spawn(async move { handler.serve(server, c2).await });
        client.write_all(b"GET / HTTP/1.1\r\n\r\n").await.unwrap();
        let mut out = vec![0u8; 128];
        let n = tokio::time::timeout(std::time::Duration::from_millis(300), client.read(&mut out))
            .await
            .unwrap()
            .unwrap();
        let s = String::from_utf8_lossy(&out[..n]);
        assert!(
            s.contains("101") || s.contains("UNEXPECTED") || s.contains("mismatch"),
            "{s}"
        );
        cancel.cancel();
    }
}

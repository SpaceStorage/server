//! SCRAM-SHA-256 (RFC 5802) for first-binary PostgreSQL auth.

use hmac::{Hmac, Mac};
use pbkdf2::pbkdf2_hmac;
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;

type HmacSha256 = Hmac<Sha256>;

const ITERATIONS: u32 = 4096;

#[derive(Debug, Clone, Default)]
pub struct UserStore {
    /// login → (secret, namespace)
    users: HashMap<String, (String, String)>,
}

impl UserStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_user(
        mut self,
        login: impl Into<String>,
        secret: impl Into<String>,
        ns: impl Into<String>,
    ) -> Self {
        self.users
            .insert(login.into(), (secret.into(), ns.into()));
        self
    }

    pub fn demo() -> Self {
        Self::new().with_user("demo", "demo", "demo")
    }

    pub fn get(&self, login: &str) -> Option<&(String, String)> {
        self.users.get(login)
    }
}

#[derive(Debug)]
pub struct ScramExchange {
    pub username: String,
    pub client_nonce: String,
    pub server_nonce: String,
    pub salt: Vec<u8>,
    pub salted_password: Vec<u8>,
    pub auth_message_prefix: String,
}

pub fn begin_scram(store: &UserStore, client_first: &str) -> Result<(ScramExchange, String), String> {
    // client-first-message: n,,n=<user>,r=<nonce>
    let bare = client_first
        .strip_prefix("n,,")
        .or_else(|| client_first.strip_prefix("y,,"))
        .unwrap_or(client_first);
    let mut username = String::new();
    let mut client_nonce = String::new();
    for part in bare.split(',') {
        if let Some(u) = part.strip_prefix("n=") {
            username = sasl_decode(u);
        } else if let Some(r) = part.strip_prefix("r=") {
            client_nonce = r.to_string();
        }
    }
    if username.is_empty() || client_nonce.is_empty() {
        return Err("invalid client-first".into());
    }
    let (secret, _ns) = store
        .get(&username)
        .cloned()
        .ok_or_else(|| "unknown user".to_string())?;

    let mut salt = vec![0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt);
    let mut server_suffix = [0u8; 18];
    rand::thread_rng().fill_bytes(&mut server_suffix);
    let server_nonce = format!(
        "{}{}",
        client_nonce,
        base64_encode(&server_suffix)
    );

    let mut salted = [0u8; 32];
    pbkdf2_hmac::<Sha256>(secret.as_bytes(), &salt, ITERATIONS, &mut salted);

    let server_first = format!(
        "r={},s={},i={}",
        server_nonce,
        base64_encode(&salt),
        ITERATIONS
    );
    let exch = ScramExchange {
        auth_message_prefix: format!("{bare},{server_first}"),
        username,
        client_nonce,
        server_nonce,
        salt,
        salted_password: salted.to_vec(),
    };
    Ok((exch, server_first))
}

pub fn finish_scram(exch: &ScramExchange, client_final: &str) -> Result<String, String> {
    // client-final: c=biws,r=<nonce>,p=<proof>
    let mut channel = String::new();
    let mut nonce = String::new();
    let mut proof_b64 = String::new();
    for part in client_final.split(',') {
        if let Some(c) = part.strip_prefix("c=") {
            channel = c.to_string();
        } else if let Some(r) = part.strip_prefix("r=") {
            nonce = r.to_string();
        } else if let Some(p) = part.strip_prefix("p=") {
            proof_b64 = p.to_string();
        }
    }
    if nonce != exch.server_nonce {
        return Err("nonce mismatch".into());
    }
    let client_final_without_proof = format!("c={channel},r={nonce}");
    let auth_message = format!("{},{}", exch.auth_message_prefix, client_final_without_proof);

    let client_key = hmac_sha256(&exch.salted_password, b"Client Key");
    let stored_key = Sha256::digest(&client_key);
    let client_signature = hmac_sha256(stored_key.as_slice(), auth_message.as_bytes());
    let proof = base64_decode(&proof_b64).map_err(|_| "bad proof".to_string())?;
    if proof.len() != client_key.len() {
        return Err("proof length".into());
    }
    let mut recovered = proof.clone();
    for (a, b) in recovered.iter_mut().zip(client_signature.iter()) {
        *a ^= *b;
    }
    let recovered_stored = Sha256::digest(&recovered);
    if recovered_stored.as_slice() != stored_key.as_slice() {
        return Err("SCRAM proof invalid".into());
    }

    let server_key = hmac_sha256(&exch.salted_password, b"Server Key");
    let server_signature = hmac_sha256(&server_key, auth_message.as_bytes());
    Ok(format!("v={}", base64_encode(&server_signature)))
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("hmac key");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn sasl_decode(s: &str) -> String {
    s.replace("=2C", ",").replace("=3D", "=")
}

fn base64_encode(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn base64_decode(s: &str) -> Result<Vec<u8>, ()> {
    fn val(c: u8) -> Result<u8, ()> {
        match c {
            b'A'..=b'Z' => Ok(c - b'A'),
            b'a'..=b'z' => Ok(c - b'a' + 26),
            b'0'..=b'9' => Ok(c - b'0' + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err(()),
        }
    }
    let bytes: Vec<u8> = s.bytes().filter(|b| *b != b'=').collect();
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        if chunk.len() < 2 {
            return Err(());
        }
        let a = val(chunk[0])? as u32;
        let b = val(chunk[1])? as u32;
        let c = if chunk.len() > 2 {
            val(chunk[2])? as u32
        } else {
            0
        };
        let d = if chunk.len() > 3 {
            val(chunk[3])? as u32
        } else {
            0
        };
        let n = (a << 18) | (b << 12) | (c << 6) | d;
        out.push(((n >> 16) & 0xff) as u8);
        if chunk.len() > 2 {
            out.push(((n >> 8) & 0xff) as u8);
        }
        if chunk.len() > 3 {
            out.push((n & 0xff) as u8);
        }
    }
    Ok(out)
}

/// Shared handle for auth.
pub type SharedUsers = Arc<UserStore>;

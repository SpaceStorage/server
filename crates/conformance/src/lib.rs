//! Conformance harness seams for the first-binary profile.
//!
//! Behavior is supplied by sibling crates (`001`–`015`). This crate owns the
//! gate tests under `--features first-binary`.

use spacestorage_config::{ValidateOptions, parse_validate};
use spacestorage_internode::{decode_frame, encode_frame, registry};
use spacestorage_node::lifecycle::NodeState;
use spacestorage_node::{Node, runtime};
use spacestorage_release_profile::{HandlerBuildSet, ReleaseProfile, TypeRequirement};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use uuid::Uuid;

/// Compile-time / status hint for the default build.
pub const RELEASE_PROFILE: &str = "first-binary";

pub fn first_binary_handlers() -> HandlerBuildSet {
    ReleaseProfile::FirstBinary.handler_set()
}

pub fn first_binary_types() -> TypeRequirement {
    ReleaseProfile::FirstBinary.type_requirement()
}

pub fn first_binary_handler_names() -> &'static [&'static str] {
    &[
        "admin",
        "admin-http",
        "internode",
        "postgresql",
        "redis",
        "replication",
        "echo",
    ]
}

/// Expanded known list when `--features handlers-complete` is enabled.
#[cfg(feature = "handlers-complete")]
pub fn handlers_complete_handler_names() -> &'static [&'static str] {
    &[
        "admin",
        "admin-http",
        "internode",
        "postgresql",
        "redis",
        "replication",
        "echo",
        "cassandra",
        "elasticsearch",
        "clickhouse",
        "clickhouse-http",
        "s3",
        "webdav",
    ]
}

/// Paths under the Spec Kit fixtures tree for first-binary starters.
pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../specs/016-mvp-and-nongoals/contracts/fixtures")
}

#[derive(Debug, Clone)]
pub struct PortMap {
    pub admin: u16,
    pub admin_http: u16,
    pub postgresql: u16,
    pub redis: u16,
    pub internode: u16,
    pub replication: u16,
}

impl PortMap {
    pub async fn ephemeral() -> Self {
        Self {
            admin: ephemeral_port().await,
            admin_http: ephemeral_port().await,
            postgresql: ephemeral_port().await,
            redis: ephemeral_port().await,
            internode: ephemeral_port().await,
            replication: ephemeral_port().await,
        }
    }
}

pub async fn ephemeral_port() -> u16 {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let p = l.local_addr().unwrap().port();
    drop(l);
    p
}

/// Materialize a starter fixture into `dir` with rewritten paths/ports/secrets.
pub struct LabNode {
    pub dir: PathBuf,
    pub conf_path: PathBuf,
    pub ports: PortMap,
    pub node: Arc<Node>,
    pub run: JoinHandle<Result<(), String>>,
    pub join_secret_path: PathBuf,
    pub data_dir: PathBuf,
}

impl LabNode {
    pub async fn shutdown(self) {
        self.node.cancel.cancel();
        self.node.force_cancel.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(5), self.run).await;
    }

    pub fn write_quorum(&self) -> String {
        self.node.config.load().query_defaults.write_quorum.clone()
    }

    pub async fn wait_ready(&self, timeout: Duration) {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if self.node.state.get() == NodeState::Ready {
                return;
            }
            if self.node.state.get() == NodeState::Failed {
                panic!("node failed before ready");
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!(
            "node did not reach ready (state={:?})",
            self.node.state.get()
        );
    }

    pub async fn membership_names(&self) -> Vec<String> {
        let g = self.node.membership.read().await;
        let Some(svc) = g.as_ref() else {
            return vec![];
        };
        let Some(view) = svc.view_snapshot() else {
            return vec![];
        };
        let mut names: Vec<_> = view.members.values().map(|m| m.node_name.clone()).collect();
        names.sort();
        names
    }

    pub async fn az_labels(&self) -> Vec<String> {
        let g = self.node.membership.read().await;
        let Some(svc) = g.as_ref() else {
            return vec![];
        };
        let Some(view) = svc.view_snapshot() else {
            return vec![];
        };
        let mut az: Vec<_> = view
            .members
            .values()
            .filter_map(|m| m.labels.get("az").cloned())
            .collect();
        az.sort();
        az.dedup();
        az
    }

    pub async fn join_secret_bytes(&self) -> Vec<u8> {
        let g = self.node.membership.read().await;
        let svc = g.as_ref().expect("membership");
        svc.accepted_join_secret().unwrap_or_else(|| {
            // Joiner before/without accepted epoch: presented secret from file.
            let text = std::fs::read_to_string(&self.join_secret_path).unwrap();
            hex::decode(text.trim()).unwrap_or_else(|_| text.trim().as_bytes().to_vec())
        })
    }

    /// Re-contact a seed so this node's membership view catches later admits (FR-009).
    pub async fn refresh_membership_from_seed(&self, seed_host: &str, seed_port: u16) {
        use spacestorage_membership::SeedEndpoint;
        let g = self.node.membership.read().await;
        let svc = g.as_ref().expect("membership");
        let seeds = [SeedEndpoint {
            name: "seed".into(),
            address: seed_host.into(),
            port: seed_port,
        }];
        svc.refresh_roster_from_seeds(&seeds)
            .await
            .expect("roster refresh");
    }

    pub async fn mint_join_token_file(&self, node_name: &str, path: &Path) -> Uuid {
        let g = self.node.membership.read().await;
        let svc = g.as_ref().expect("membership");
        let token = svc
            .mint_join_token(node_name, None, Duration::from_secs(12 * 3600))
            .expect("mint join token");
        std::fs::write(path, token.token_id.to_string()).unwrap();
        token.token_id
    }
}

fn rewrite_fixture(
    template: &str,
    dir: &Path,
    ports: &PortMap,
    seed_internode: Option<(String, u16)>,
    join_token_file: Option<&Path>,
) -> String {
    let admin_token = dir.join("admin.token");
    let master_key = dir.join("master.key");
    let join_secret = dir.join("join.secret");
    let data_dir = dir.join("data");
    let drive = data_dir.join("nvme0");
    std::fs::create_dir_all(&drive).unwrap();
    if !admin_token.exists() {
        std::fs::write(&admin_token, "lab-admin-token\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = std::fs::metadata(&admin_token).unwrap().permissions();
            p.set_mode(0o600);
            std::fs::set_permissions(&admin_token, p).unwrap();
        }
    }
    if !master_key.exists() {
        spacestorage_crypto::MasterKey::generate_and_write_blocking(&master_key).unwrap();
    }
    // join.secret may be copied from bootstrap; create placeholder only if absent.
    if !join_secret.exists() {
        std::fs::write(&join_secret, "00").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = std::fs::metadata(&join_secret).unwrap().permissions();
            p.set_mode(0o600);
            std::fs::set_permissions(&join_secret, p).unwrap();
        }
    }

    let mut out = template
        .replace(
            "/etc/spacestorage/admin.token",
            admin_token.to_str().unwrap(),
        )
        .replace("/etc/spacestorage/master.key", master_key.to_str().unwrap())
        .replace(
            "/etc/spacestorage/join.secret",
            join_secret.to_str().unwrap(),
        )
        .replace("/var/lib/spacestorage/db-1", data_dir.to_str().unwrap())
        .replace("/var/lib/spacestorage/db-a", data_dir.to_str().unwrap())
        .replace("/var/lib/spacestorage/db-b", data_dir.to_str().unwrap())
        .replace("/var/lib/spacestorage/db-c", data_dir.to_str().unwrap());

    // Port rewrites — match fixture defaults by handler block.
    out = rewrite_port(&out, "handler admin;", ports.admin);
    out = rewrite_port(&out, "handler admin-http;", ports.admin_http);
    out = rewrite_port(&out, "handler postgresql;", ports.postgresql);
    out = rewrite_port(&out, "handler redis;", ports.redis);
    out = rewrite_port(&out, "handler internode;", ports.internode);
    out = rewrite_port(&out, "handler replication;", ports.replication);

    if let Some((addr, port)) = seed_internode {
        // Replace seeds { address …; port …; }
        if let Some(start) = out.find("seeds {") {
            if let Some(end_rel) = out[start..].find('}') {
                let end = start + end_rel;
                let replacement =
                    format!("seeds {{\n    name seed;\n    address {addr};\n    port {port};\n  ");
                out.replace_range(start..end, &replacement);
            }
        }
    }

    if let Some(tf) = join_token_file {
        // Inject join_token_file into cluster block if missing.
        if !out.contains("join_token_file") {
            out = out.replace(
                "token_file ",
                &format!("join_token_file {};\n  token_file ", tf.display()),
            );
        }
    }

    out
}

fn rewrite_port(conf: &str, handler_marker: &str, port: u16) -> String {
    // Find the entrypoint block containing handler_marker and rewrite its `port N;`.
    let Some(hpos) = conf.find(handler_marker) else {
        return conf.to_string();
    };
    // Walk backward to `entrypoint` and forward through a few lines for `port`.
    let before = &conf[..hpos];
    let ep_start = before.rfind("entrypoint").unwrap_or(0);
    let after = &conf[hpos..];
    let ep_end = after.find('}').map(|i| hpos + i).unwrap_or(conf.len());
    let mut block = conf[ep_start..=ep_end].to_string();
    if let Some(p) = block.find("port ") {
        let rest = &block[p + 5..];
        let num_end = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        block.replace_range(p + 5..p + 5 + num_end, &port.to_string());
    }
    let mut out = conf.to_string();
    out.replace_range(ep_start..=ep_end, &block);
    out
}

pub async fn boot_from_fixture(
    fixture_name: &str,
    work: PathBuf,
    ports: PortMap,
    seed_internode: Option<(String, u16)>,
    join_token_file: Option<PathBuf>,
    shared_join_secret: Option<PathBuf>,
) -> LabNode {
    std::fs::create_dir_all(&work).unwrap();
    let template = std::fs::read_to_string(fixtures_dir().join(fixture_name)).unwrap();
    if let Some(src) = shared_join_secret.as_ref() {
        let dst = work.join("join.secret");
        std::fs::copy(src, &dst).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = std::fs::metadata(&dst).unwrap().permissions();
            p.set_mode(0o600);
            std::fs::set_permissions(&dst, p).unwrap();
        }
    }
    let text = rewrite_fixture(
        &template,
        &work,
        &ports,
        seed_internode,
        join_token_file.as_deref(),
    );
    let conf_path = work.join("node.conf");
    std::fs::write(&conf_path, &text).unwrap();

    let handlers = first_binary_handler_names();
    let (cfg, _) = parse_validate(
        &text,
        &conf_path,
        &[],
        handlers,
        ValidateOptions::first_binary(),
    )
    .unwrap_or_else(|e| panic!("validate {fixture_name}: {e:?}"));

    let (threads, source) = runtime::resolve_worker_threads(cfg.threads);
    let node = Node::boot_first_binary(conf_path.clone(), cfg, threads, source);
    let run = {
        let n = node.clone();
        tokio::spawn(async move { n.run().await })
    };
    LabNode {
        join_secret_path: work.join("join.secret"),
        data_dir: work.join("data"),
        dir: work,
        conf_path,
        ports,
        node,
        run,
    }
}

/// G1: one-node starter with write_quorum ONE.
pub async fn boot_one_node(root: &Path) -> LabNode {
    let work = root.join("one");
    let ports = PortMap::ephemeral().await;
    let lab = boot_from_fixture("first-binary-one-node.conf", work, ports, None, None, None).await;
    lab.wait_ready(Duration::from_secs(10)).await;
    lab
}

/// G2: bootstrap A then join B and C with one-time tokens.
pub async fn boot_three_node(root: &Path) -> (LabNode, LabNode, LabNode) {
    let ports_a = PortMap::ephemeral().await;
    let a = boot_from_fixture(
        "first-binary-three-node-a.conf",
        root.join("a"),
        ports_a.clone(),
        None,
        None,
        None,
    )
    .await;
    a.wait_ready(Duration::from_secs(10)).await;

    let token_b = root.join("token-b");
    let token_c = root.join("token-c");
    a.mint_join_token_file("db-b", &token_b).await;
    a.mint_join_token_file("db-c", &token_c).await;

    let seed = ("127.0.0.1".into(), a.ports.internode);
    let secret = a.join_secret_path.clone();

    let ports_b = PortMap::ephemeral().await;
    let b = boot_from_fixture(
        "first-binary-three-node-b.conf",
        root.join("b"),
        ports_b,
        Some(seed.clone()),
        Some(token_b),
        Some(secret.clone()),
    )
    .await;
    b.wait_ready(Duration::from_secs(15)).await;

    let ports_c = PortMap::ephemeral().await;
    let c = boot_from_fixture(
        "first-binary-three-node-c.conf",
        root.join("c"),
        ports_c,
        Some(seed),
        Some(token_c),
        Some(secret),
    )
    .await;
    c.wait_ready(Duration::from_secs(15)).await;

    // Wait until every node sees the same three-member view (FR-009).
    // Without Raft, earlier joiners refresh from the seed after later admits.
    let expected = vec![
        String::from("db-a"),
        String::from("db-b"),
        String::from("db-c"),
    ];
    let seed_host = "127.0.0.1";
    let seed_port = a.ports.internode;
    let start = std::time::Instant::now();
    loop {
        let _ = b.refresh_membership_from_seed(seed_host, seed_port).await;
        let _ = c.refresh_membership_from_seed(seed_host, seed_port).await;
        let ma = a.membership_names().await;
        let mb = b.membership_names().await;
        let mc = c.membership_names().await;
        if ma == expected && mb == expected && mc == expected {
            break;
        }
        if start.elapsed() > Duration::from_secs(10) {
            panic!("membership did not converge: a={ma:?} b={mb:?} c={mc:?}");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    (a, b, c)
}

pub async fn scrape_metrics(http_port: u16) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", http_port))
        .await
        .expect("metrics connect");
    stream
        .write_all(b"GET /metrics HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    String::from_utf8_lossy(&buf).into_owned()
}

pub async fn tcp_bound(port: u16) -> bool {
    TcpStream::connect(("127.0.0.1", port)).await.is_ok()
}

/// Contact a replication entrypoint; return whether ACK is durable.
pub async fn replication_durable_ack(addr: &str, secret: &[u8], payload: &[u8]) -> bool {
    let mut stream = match TcpStream::connect(addr).await {
        Ok(s) => s,
        Err(_) => return false,
    };
    if stream.write_all(&encode_frame(0, secret)).await.is_err() {
        return false;
    }
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    // auth ack
    loop {
        match tokio::time::timeout(Duration::from_secs(2), stream.read(&mut tmp)).await {
            Ok(Ok(0)) | Ok(Err(_)) | Err(_) => return false,
            Ok(Ok(n)) => {
                buf.extend_from_slice(&tmp[..n]);
                if let Ok((frame, consumed)) = decode_frame(&buf) {
                    buf.drain(..consumed);
                    if frame.msg_type != registry::MSG_ACK {
                        return false;
                    }
                    break;
                }
            }
        }
    }
    if stream
        .write_all(&encode_frame(registry::MSG_ACK, payload))
        .await
        .is_err()
    {
        return false;
    }
    buf.clear();
    loop {
        match tokio::time::timeout(Duration::from_secs(3), stream.read(&mut tmp)).await {
            Ok(Ok(0)) | Ok(Err(_)) | Err(_) => return false,
            Ok(Ok(n)) => {
                buf.extend_from_slice(&tmp[..n]);
                if let Ok((frame, consumed)) = decode_frame(&buf) {
                    let _ = consumed;
                    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&frame.payload) {
                        return v.get("durable").and_then(|d| d.as_bool()) == Some(true)
                            && v.get("ok").and_then(|d| d.as_bool()) != Some(false);
                    }
                    return false;
                }
            }
        }
    }
}

/// Leaderless TWO write via placement fan-out against live replication ports.
pub async fn quorum_two_write(
    replicas: &[(String, u16)],
    secret: &[u8],
    payload: &[u8],
) -> Result<u32, String> {
    use spacestorage_placement::{
        FanoutAck, FanoutReplica, FanoutWrite, QuorumLevel, fanout_write,
    };

    let fanout_replicas: Vec<FanoutReplica> = replicas
        .iter()
        .map(|(host, port)| FanoutReplica {
            node: format!("{host}:{port}"),
            quorum_domain: "lab".into(),
        })
        .collect();
    let write = FanoutWrite {
        container_id: "g6".into(),
        key: b"k".to_vec(),
        value: payload.to_vec(),
        stamp_physical: 1,
        stamp_logical: 0,
    };
    // fanout_write is sync; gather async acks first then score.
    let mut acks: BTreeMap<String, FanoutAck> = BTreeMap::new();
    for (host, port) in replicas {
        let addr = format!("{host}:{port}");
        let ok = replication_durable_ack(&addr, secret, payload).await;
        acks.insert(
            addr.clone(),
            FanoutAck {
                replica: addr,
                ok,
                durable: ok,
                value: None,
                stamp_physical: None,
                stamp_logical: None,
            },
        );
    }
    fanout_write(
        "lab",
        "lab",
        true,
        &fanout_replicas,
        &write,
        QuorumLevel::Two,
        |r| {
            acks.get(&r.node).cloned().unwrap_or(FanoutAck {
                replica: r.node.clone(),
                ok: false,
                durable: false,
                value: None,
                stamp_physical: None,
                stamp_logical: None,
            })
        },
    )
    .map(|o| o.durable_acks)
    .map_err(|e| e.to_string())
}

/// Validate-only helpers for invalid fixtures (G1 omitted-transport / G8 cassandra).
pub fn validate_fixture(rel: &str) -> Result<(), Vec<spacestorage_config::ConfigError>> {
    let path = fixtures_dir().join(rel);
    let text = std::fs::read_to_string(&path).unwrap();
    parse_validate(
        &text,
        &path,
        &[],
        first_binary_handler_names(),
        ValidateOptions {
            check_secrets_readable: false,
            ..ValidateOptions::first_binary()
        },
    )
    .map(|_| ())
}

/// Whether the in-process harness can boot nodes (always true once this module links).
/// Prefer calling `boot_one_node` / `boot_three_node` / `validate_fixture` directly in gates.
#[deprecated(note = "use boot_one_node / boot_three_node / validate_fixture")]
pub fn in_process_cluster_ready() -> bool {
    true
}

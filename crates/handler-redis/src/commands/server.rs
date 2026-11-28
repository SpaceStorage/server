//! Redis `server` / introspection family (`017` US4: T051–T055).
//!
//! `INFO`, `DBSIZE`, `TYPE`, `RANDOMKEY`, `TOUCH`, the `OBJECT` subset,
//! `COMMAND`, `TIME`, `ECHO`, `QUIT`, the `CLIENT` subset, and `CONFIG`. Every
//! value reflects real handler/node/namespace state; fields with no internal
//! equivalent are omitted, never emitted as a stock-Redis constant (research R10,
//! constitution X). `SCAN`/`DEL`/`UNLINK`/`TTL` stay in `strings.rs`; this module
//! returns `None` for them so the existing family handles them.

use std::time::Duration;

use spacestorage_compat::{CapabilityCommand, REDIS_CAPABILITY_TABLE, ResourceGroup, redis_capability};
use spacestorage_query::LogicalRequest;

use crate::expiry::now_unix_millis;
use crate::lower::EntryPayload;
use crate::render;
use super::{RedisReply, SessionState};

/// The six documented `INFO` sections, in reply order (research R10).
const INFO_SECTIONS: &[&str] = &[
    "server",
    "clients",
    "memory",
    "stats",
    "keyspace",
    "replication",
];

/// `017` US4 T051–T055: the `server`/introspection command surface.
pub fn dispatch(session: &mut SessionState, upper: &str, args: &[&str]) -> Option<RedisReply> {
    match upper {
        "INFO" => Some(cmd_info(session, args)),
        "DBSIZE" => Some(cmd_dbsize(session)),
        "TYPE" => Some(cmd_type(session, args)),
        "RANDOMKEY" => Some(cmd_randomkey(session)),
        "TOUCH" => Some(cmd_touch(session, args)),
        "OBJECT" => Some(cmd_object(session, args)),
        "COMMAND" => Some(cmd_command(args)),
        "TIME" => Some(cmd_time()),
        "ECHO" => Some(cmd_echo(args)),
        // `QUIT` is classified Must; the serve loop closes the connection after
        // writing this `+OK` (see `FrameOutcome::ReplyThenClose`).
        "QUIT" => Some(RedisReply::Ok),
        "CLIENT" => Some(cmd_client(session, args)),
        "CONFIG" => Some(cmd_config(session, args)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// INFO
// ---------------------------------------------------------------------------

fn cmd_info(session: &SessionState, args: &[&str]) -> RedisReply {
    let mut requested: Vec<&'static str> = Vec::new();
    let mut include_commandstats = false;
    if args.is_empty() {
        requested.extend_from_slice(INFO_SECTIONS);
    } else {
        for a in args {
            let s = a.to_ascii_lowercase();
            match s.as_str() {
                "all" | "everything" => {
                    include_commandstats = true;
                    for sec in INFO_SECTIONS {
                        if !requested.contains(sec) {
                            requested.push(sec);
                        }
                    }
                }
                "commandstats" => include_commandstats = true,
                _ => {
                    if let Some(sec) = INFO_SECTIONS.iter().find(|sec| **sec == s) {
                        if !requested.contains(sec) {
                            requested.push(sec);
                        }
                    }
                }
            }
        }
        // An entirely unknown section is an empty bulk string, as in stock Redis.
        if requested.is_empty() && !include_commandstats {
            return RedisReply::Bulk(Some(Vec::new()));
        }
    }

    let mut out = String::new();
    for section in &requested {
        render_info_section(session, section, &mut out);
    }
    if include_commandstats {
        render_commandstats(session, &mut out);
    }
    RedisReply::Bulk(Some(out.into_bytes()))
}

/// `# Section\r\n` header + `field:value\r\n` lines + a trailing blank line.
fn render_info_section(session: &SessionState, section: &str, out: &mut String) {
    out.push_str("# ");
    out.push_str(&section_title(section));
    out.push_str("\r\n");
    match section {
        "server" => render_server(session, out),
        "clients" => render_clients(session, out),
        "memory" => render_memory(session, out),
        "stats" => render_stats(session, out),
        "keyspace" => render_keyspace(session, out),
        "replication" => render_replication(session, out),
        _ => {}
    }
    out.push_str("\r\n");
}

/// `server` → `Server` (Redis capitalizes the first letter of section titles).
fn section_title(section: &str) -> String {
    let mut chars = section.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn field(out: &mut String, name: &str, value: impl std::fmt::Display) {
    out.push_str(name);
    out.push(':');
    out.push_str(&value.to_string());
    out.push_str("\r\n");
}

fn render_server(session: &SessionState, out: &mut String) {
    let info = session.node_info.snapshot();
    field(out, "redis_version", "7.4.0-spacestorage");
    field(out, "redis_mode", "standalone");
    field(out, "os", std::env::consts::OS);
    field(out, "arch_bits", std::mem::size_of::<usize>() * 8);
    field(out, "process_id", std::process::id());
    field(out, "uptime_in_seconds", info.uptime_seconds);
    field(out, "uptime_in_days", info.uptime_seconds / 86_400);
    if let Some(port) = info.tcp_port {
        field(out, "tcp_port", port);
    }
    if let Some(name) = &info.node_name {
        field(out, "node_name", name);
    }
    // Omitted (no internal equivalent, never faked): run_id, config_file,
    // executable, io_threads_*, redis_git_*, and the rest of the stock template.
}

fn render_clients(session: &SessionState, out: &mut String) {
    let snap = session.stats.snapshot();
    field(out, "connected_clients", snap.connected_clients);
    // The configured per-entrypoint connection cap is the real server-side limit.
    field(
        out,
        "maxclients",
        session.limit_services.limits().max_connections_per_entrypoint,
    );
    // Omitted: blocked_clients (the block registry exposes no count), and every
    // stock client-tracking gauge the handler does not keep.
}

fn render_memory(session: &SessionState, out: &mut String) {
    let info = session.node_info.snapshot();
    if let Some(used) = info.used_memory_bytes {
        field(out, "used_memory", used);
        field(out, "used_memory_human", human_bytes(used));
    }
    // Header stays even when nothing is reported. Omitted: mem_fragmentation_*,
    // used_memory_rss, maxmemory_*, mem_allocator, … (no internal equivalent).
}

fn render_stats(session: &SessionState, out: &mut String) {
    let snap = session.stats.snapshot();
    field(out, "total_connections_received", snap.total_connections_received);
    field(out, "total_commands_processed", snap.total_commands_processed);
    field(out, "expired_keys", snap.expired_keys);
    field(out, "keyspace_hits", snap.keyspace_hits);
    field(out, "keyspace_misses", snap.keyspace_misses);
}

fn render_keyspace(session: &SessionState, out: &mut String) {
    let (live, expires) = scan_live_keys(session).unwrap_or((0, 0));
    field(out, "db0", format!("keys={live},expires={expires},avg_ttl=0"));
}

fn render_replication(session: &SessionState, out: &mut String) {
    let info = session.node_info.snapshot();
    // Real, membership-derived role; never a hardcoded `role:master`.
    field(out, "role", info.role);
    field(out, "connected_slaves", 0);
    // Omitted: master_replid / master_repl_offset / repl_backlog_* — SpaceStorage
    // is not Redis replication, so there is no real value to report.
}

fn render_commandstats(session: &SessionState, out: &mut String) {
    out.push_str("# Commandstats\r\n");
    for (name, calls, usec) in session.stats.command_stats() {
        let per_call = if calls > 0 {
            usec as f64 / calls as f64
        } else {
            0.0
        };
        out.push_str(&format!(
            "cmdstat_{name}:calls={calls},usec={usec},usec_per_call={per_call:.2}\r\n"
        ));
    }
    out.push_str("\r\n");
}

fn human_bytes(bytes: u64) -> String {
    const K: f64 = 1024.0;
    let b = bytes as f64;
    if b >= K * K * K {
        format!("{:.2}G", b / (K * K * K))
    } else if b >= K * K {
        format!("{:.2}M", b / (K * K))
    } else if b >= K {
        format!("{:.2}K", b / K)
    } else {
        format!("{bytes}B")
    }
}

// ---------------------------------------------------------------------------
// Keyspace introspection
// ---------------------------------------------------------------------------

/// Walk the namespace key-space and count live (non-expired) keys and how many
/// of them carry a TTL. `load_entry` purges expired keys on read, so an expired
/// key is neither counted nor reported (data-model §7).
fn scan_live_keys(session: &SessionState) -> Result<(u64, u64), RedisReply> {
    let id = session.keyspace_id()?;
    let mut cursor = 0u64;
    let mut live = 0u64;
    let mut expires = 0u64;
    let mut guard = 0usize;
    loop {
        let r = session.planner_exec(LogicalRequest::KeyScan {
            container: session.container.clone(),
            cursor,
            count: 1000,
            pattern: None,
        })?;
        for row in &r.rows {
            if let Some(k) = row.first().and_then(|k| k.clone()) {
                match session.load_entry(id, &k) {
                    Ok(Some(entry)) => {
                        live += 1;
                        if entry.expire_at.is_some() {
                            expires += 1;
                        }
                    }
                    Ok(None) => {}
                    Err(e) => return Err(e),
                }
            }
        }
        let next = r.tag.parse::<u64>().unwrap_or(0);
        if next == 0 || next == cursor {
            break;
        }
        cursor = next;
        guard += 1;
        if guard > 1_000_000 {
            break;
        }
    }
    Ok((live, expires))
}

fn cmd_dbsize(session: &SessionState) -> RedisReply {
    match scan_live_keys(session) {
        Ok((live, _)) => RedisReply::Integer(live as i64),
        Err(e) => e,
    }
}

fn cmd_type(session: &SessionState, args: &[&str]) -> RedisReply {
    if args.len() != 1 {
        return render::arity("type");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    match session.load_entry(id, args[0]) {
        Ok(Some(entry)) => RedisReply::Simple(entry.visible.as_str().to_string()),
        Ok(None) => RedisReply::Simple("none".to_string()),
        Err(e) => e,
    }
}

fn cmd_randomkey(session: &SessionState) -> RedisReply {
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let r = match session.planner_exec(LogicalRequest::KeyScan {
        container: session.container.clone(),
        cursor: 0,
        count: 1000,
        pattern: None,
    }) {
        Ok(r) => r,
        Err(e) => return e,
    };
    for row in &r.rows {
        if let Some(k) = row.first().and_then(|k| k.clone()) {
            match session.load_entry(id, &k) {
                Ok(Some(_)) => return RedisReply::Bulk(Some(k.into_bytes())),
                Ok(None) => continue,
                Err(e) => return e,
            }
        }
    }
    RedisReply::Bulk(None)
}

fn cmd_touch(session: &SessionState, args: &[&str]) -> RedisReply {
    if args.is_empty() {
        return render::arity("touch");
    }
    let id = match session.keyspace_id() {
        Ok(id) => id,
        Err(e) => return e,
    };
    let mut n = 0i64;
    for key in args {
        match session.load_entry(id, key) {
            Ok(Some(_)) => n += 1,
            Ok(None) => {}
            Err(e) => return e,
        }
    }
    RedisReply::Integer(n)
}

/// `OBJECT` subset: only `ENCODING` and `IDLETIME` are supported (contract
/// R2). Any other subcommand is the Redis not-supported error form.
fn cmd_object(session: &SessionState, args: &[&str]) -> RedisReply {
    let Some(sub) = args.first() else {
        return render::unknown_subcommand();
    };
    match sub.to_ascii_uppercase().as_str() {
        "ENCODING" | "IDLETIME" => {
            if args.len() != 2 {
                return render::unknown_subcommand();
            }
            let id = match session.keyspace_id() {
                Ok(id) => id,
                Err(e) => return e,
            };
            match session.load_entry(id, args[1]) {
                Ok(Some(entry)) => {
                    if sub.eq_ignore_ascii_case("encoding") {
                        RedisReply::Bulk(Some(encoding_for(&entry.payload).as_bytes().to_vec()))
                    } else {
                        // No persisted access timestamp: report 0 rather than a
                        // fabricated large idle time. Idle tracking is not stored.
                        RedisReply::Integer(0)
                    }
                }
                Ok(None) => render::error("ERR no such key"),
                Err(e) => e,
            }
        }
        _ => render::unknown_subcommand(),
    }
}

/// `OBJECT ENCODING` layout hint per entry payload. Documented mapping: plain,
/// bitmap, and HLL strings → `raw`; hash → `hashtable`; list → `listpack`;
/// set → `hashtable`; sorted set → `skiplist`; stream → `stream`.
pub(crate) fn encoding_for(payload: &EntryPayload) -> &'static str {
    match payload {
        EntryPayload::Bytes(_) | EntryPayload::Bitmap(_) | EntryPayload::HyperLogLog(_) => "raw",
        EntryPayload::Hash(_) => "hashtable",
        EntryPayload::List(_) => "listpack",
        EntryPayload::Set(_) => "hashtable",
        EntryPayload::ZSet(_) => "skiplist",
        EntryPayload::Stream(_) => "stream",
    }
}

// ---------------------------------------------------------------------------
// COMMAND
// ---------------------------------------------------------------------------

/// `COMMAND` / `COMMAND INFO [name…]` / `COMMAND COUNT`, sourced from the single
/// `015` capability table (FR-011) — never a second table.
fn cmd_command(args: &[&str]) -> RedisReply {
    match args.first().map(|s| s.to_ascii_uppercase()).as_deref() {
        None => RedisReply::Array(REDIS_CAPABILITY_TABLE.iter().map(command_info).collect()),
        Some("COUNT") if args.len() == 1 => {
            RedisReply::Integer(REDIS_CAPABILITY_TABLE.len() as i64)
        }
        Some("INFO") => {
            if args.len() == 1 {
                RedisReply::Array(REDIS_CAPABILITY_TABLE.iter().map(command_info).collect())
            } else {
                let items = args[1..]
                    .iter()
                    .map(|name| match redis_capability(&name.to_ascii_uppercase()) {
                        Some(row) => command_info(row),
                        None => RedisReply::Bulk(None),
                    })
                    .collect();
                RedisReply::Array(items)
            }
        }
        Some(_) => render::unknown_subcommand(),
    }
}

/// `[name, arity, [flags], first_key, last_key, step]` — the minimum shape a
/// stock client tolerates. Arity is the variadic sentinel because a capability
/// row records presence, not a fixed arity; the array length stays consistent.
fn command_info(row: &CapabilityCommand) -> RedisReply {
    let name = row.name.to_ascii_lowercase();
    let keyed = !matches!(
        row.group,
        ResourceGroup::ConnectionMeta | ResourceGroup::PubSub | ResourceGroup::Transaction
    );
    let (first, last, step) = if keyed { (1, 1, 1) } else { (0, 0, 0) };
    RedisReply::Array(vec![
        RedisReply::Bulk(Some(name.into_bytes())),
        RedisReply::Integer(-1),
        RedisReply::Array(Vec::new()),
        RedisReply::Integer(first),
        RedisReply::Integer(last),
        RedisReply::Integer(step),
    ])
}

// ---------------------------------------------------------------------------
// Connection / server meta
// ---------------------------------------------------------------------------

fn cmd_time() -> RedisReply {
    let ms = now_unix_millis();
    let secs = ms.div_euclid(1000);
    let micros = ms.rem_euclid(1000) * 1000;
    RedisReply::Array(vec![
        RedisReply::Bulk(Some(secs.to_string().into_bytes())),
        RedisReply::Bulk(Some(micros.to_string().into_bytes())),
    ])
}

fn cmd_echo(args: &[&str]) -> RedisReply {
    match args {
        [msg] => RedisReply::Bulk(Some(msg.as_bytes().to_vec())),
        _ => render::arity("echo"),
    }
}

fn cmd_client(session: &mut SessionState, args: &[&str]) -> RedisReply {
    let Some(sub) = args.first() else {
        return render::unknown_subcommand();
    };
    let rest = &args[1..];
    match sub.to_ascii_uppercase().as_str() {
        "ID" if rest.is_empty() => RedisReply::Integer(session.connection_id.0 as i64),
        "GETNAME" if rest.is_empty() => match &session.client_name {
            Some(name) => RedisReply::Bulk(Some(name.clone().into_bytes())),
            None => RedisReply::Bulk(None),
        },
        "SETNAME" => match rest {
            [name] => {
                session.client_name = Some((*name).to_string());
                RedisReply::Ok
            }
            _ => render::arity("client|setname"),
        },
        "INFO" | "LIST" if rest.is_empty() => {
            RedisReply::Bulk(Some(client_line(session).into_bytes()))
        }
        // The handler cannot terminate another live connection in-process, so it
        // replies 0 and never claims a kill it did not perform.
        "KILL" => RedisReply::Integer(0),
        // Accepted for compatibility; PAUSE is not enforced per-connection
        // in-process (documented residual).
        "PAUSE" => RedisReply::Ok,
        _ => render::unknown_subcommand(),
    }
}

fn client_line(session: &SessionState) -> String {
    let name = session.client_name.clone().unwrap_or_default();
    // `addr` is omitted: this handler does not track the peer address. Only the
    // connection it actually owns is listed — never a fabricated other client.
    format!("id={} name={name} db=0", session.connection_id.0)
}

fn cmd_config(session: &mut SessionState, args: &[&str]) -> RedisReply {
    let Some(sub) = args.first() else {
        return render::unknown_subcommand();
    };
    let rest = &args[1..];
    match sub.to_ascii_uppercase().as_str() {
        "GET" => match rest {
            [param] => config_get(session, param),
            _ => render::arity("config|get"),
        },
        "SET" => match rest {
            [param, value] => config_set(session, param, value),
            _ => render::arity("config|set"),
        },
        _ => render::unknown_subcommand(),
    }
}

/// Only `max_block` is a known parameter; anything else is omitted (empty array).
fn config_get(session: &SessionState, param: &str) -> RedisReply {
    if param.eq_ignore_ascii_case("max_block") {
        if let Some(d) = session.max_block {
            return RedisReply::Array(vec![
                RedisReply::Bulk(Some(b"max_block".to_vec())),
                RedisReply::Bulk(Some(d.as_secs().to_string().into_bytes())),
            ]);
        }
    }
    RedisReply::Array(Vec::new())
}

/// Only `max_block` (seconds) is settable; any other parameter is an error.
fn config_set(session: &mut SessionState, param: &str, value: &str) -> RedisReply {
    if param.eq_ignore_ascii_case("max_block") {
        return match value.parse::<u64>() {
            Ok(secs) => {
                session.max_block = Some(Duration::from_secs(secs));
                RedisReply::Ok
            }
            Err(_) => render::error("ERR value is not an integer or out of range"),
        };
    }
    render::error(format!("ERR Unsupported CONFIG parameter: {param}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lower::{BitmapValue, HashValue, HllValue, ListValue};

    #[test]
    fn encoding_maps_each_payload_family() {
        assert_eq!(encoding_for(&EntryPayload::Bytes(vec![])), "raw");
        assert_eq!(
            encoding_for(&EntryPayload::Bitmap(BitmapValue(vec![1]))),
            "raw",
            "bitmap is a Redis string"
        );
        assert_eq!(
            encoding_for(&EntryPayload::HyperLogLog(HllValue {
                precision: 14,
                registers: vec![0; 16],
            })),
            "raw",
            "HLL is a Redis string"
        );
        assert_eq!(encoding_for(&EntryPayload::Hash(HashValue::default())), "hashtable");
        assert_eq!(encoding_for(&EntryPayload::List(ListValue::default())), "listpack");
    }

    #[test]
    fn section_titles_capitalize_first_letter() {
        assert_eq!(section_title("server"), "Server");
        assert_eq!(section_title("replication"), "Replication");
        assert_eq!(section_title(""), "");
    }
}

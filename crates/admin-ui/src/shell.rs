//! Shell section dispatch, pagination (R5), and affordance filtering.
//! Contracts: `specs/018-admin-ui-modernization/contracts/shell-http.md`.

use serde::{Deserialize, Serialize};

pub const DEFAULT_LIMIT: u32 = 50;
pub const MAX_LIMIT: u32 = 100;
pub const PAGE_BYTE_CAP: usize = 256 * 1024;
pub const DESCRIPTOR_SCAN_CAP: usize = 10_000;

/// Section identifiers per shell-http contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellSection {
    Overview,
    Namespaces,
    Nodes,
    Monitoring,
    Jobs,
    Audit,
    Map,
    Browse,
}

impl ShellSection {
    pub fn from_path(path: &str) -> Option<Self> {
        let path = path.trim_end_matches('/');
        match path {
            "/ui/overview" => Some(Self::Overview),
            p if p == "/ui/namespaces" || p.starts_with("/ui/namespaces/") => {
                Some(Self::Namespaces)
            }
            p if p == "/ui/nodes" || p.starts_with("/ui/nodes/") => Some(Self::Nodes),
            p if p == "/ui/monitoring" || p.starts_with("/ui/monitoring/") => {
                Some(Self::Monitoring)
            }
            p if p == "/ui/jobs" || p.starts_with("/ui/jobs/") => Some(Self::Jobs),
            "/ui/audit" => Some(Self::Audit),
            "/ui/map" => Some(Self::Map),
            "/ui/browse" => Some(Self::Browse),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Namespaces => "namespaces",
            Self::Nodes => "nodes",
            Self::Monitoring => "monitoring",
            Self::Jobs => "jobs",
            Self::Audit => "audit",
            Self::Map => "map",
            Self::Browse => "browse",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Namespaces => "Namespaces",
            Self::Nodes => "Nodes",
            Self::Monitoring => "Monitoring",
            Self::Jobs => "Jobs",
            Self::Audit => "Audit",
            Self::Map => "Cluster map",
            Self::Browse => "Browse",
        }
    }

    pub fn affordance_key(self) -> Option<&'static str> {
        match self {
            Self::Overview | Self::Namespaces | Self::Nodes | Self::Jobs => None,
            Self::Monitoring => Some("metrics_read"),
            Self::Audit => Some("audit_read"),
            Self::Map => Some("map"),
            Self::Browse => Some("browse"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UiAffordances {
  pub namespace_write: bool,
  pub quota_replace: bool,
  pub config_reload: bool,
  pub cluster_stop: bool,
  pub node_drain: bool,
  pub job_control: bool,
  pub audit_read: bool,
  pub metrics_read: bool,
  pub map: bool,
  pub browse: bool,
}

/// Sections visible in nav for the given affordances (US1 T026).
pub fn nav_sections(aff: &UiAffordances) -> Vec<ShellSection> {
    use ShellSection::*;
    let all = [
        Overview,
        Namespaces,
        Nodes,
        Monitoring,
        Jobs,
        Audit,
        Map,
        Browse,
    ];
    all.into_iter()
        .filter(|s| match s.affordance_key() {
            None => true,
            Some("metrics_read") => aff.metrics_read,
            Some("audit_read") => aff.audit_read,
            Some("map") => aff.map,
            Some("browse") => aff.browse,
            Some(_) => true,
        })
        .collect()
}

/// Namespace in `/ui/monitoring/namespaces/{name}` when present.
pub fn monitoring_namespace_from_path(path: &str) -> Option<String> {
    let path = path.trim_end_matches('/');
    let rest = path.strip_prefix("/ui/monitoring/namespaces/")?;
    if rest.is_empty() || rest.contains('/') {
        return None;
    }
    Some(rest.to_string())
}

/// Namespace in `/ui/namespaces/{name}` when present.
pub fn namespace_from_path(path: &str) -> Option<String> {
    let path = path.trim_end_matches('/');
    let rest = path.strip_prefix("/ui/namespaces/")?;
    if rest.is_empty() || rest.contains('/') {
        return None;
    }
    Some(rest.to_string())
}

/// Direct link refusal for namespace-scoped principals (US1 T031).
pub fn check_namespace_path(path: &str, scope: &str, allowed: &[String]) -> Result<(), &'static str> {
    if scope == "cluster" {
        return Ok(());
    }
    let Some(ns) = namespace_from_path(path)
        .or_else(|| monitoring_namespace_from_path(path))
    else {
        return Ok(());
    };
    if allowed.iter().any(|n| n == &ns) {
        Ok(())
    } else {
        Err("You do not have access to this namespace.")
    }
}

/// Whether a section is meaningful for namespace scope (nav may hide cluster-only affordances).
pub fn section_allowed_for_scope(section: ShellSection, scope: &str) -> bool {
    if scope == "cluster" {
        return true;
    }
    match section {
        ShellSection::Overview
        | ShellSection::Namespaces
        | ShellSection::Nodes
        | ShellSection::Jobs
        | ShellSection::Map
        | ShellSection::Browse => true,
        ShellSection::Monitoring | ShellSection::Audit => false,
    }
}

impl UiAffordances {
    pub fn cluster_admin() -> Self {
        Self {
            namespace_write: true,
            quota_replace: true,
            config_reload: true,
            cluster_stop: true,
            node_drain: true,
            job_control: true,
            audit_read: true,
            metrics_read: true,
            map: true,
            browse: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PageParams {
    pub limit: u32,
    pub cursor: Option<String>,
}

impl Default for PageParams {
    fn default() -> Self {
        Self {
            limit: DEFAULT_LIMIT,
            cursor: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum LimitError {
    Exceeded,
}

/// Parse `limit` query: default 50, reject outside 1–100.
pub fn parse_limit(raw: Option<&str>) -> Result<u32, LimitError> {
    let limit = match raw {
        None | Some("") => DEFAULT_LIMIT,
        Some(s) => s.parse::<u32>().map_err(|_| LimitError::Exceeded)?,
    };
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(LimitError::Exceeded);
    }
    Ok(limit)
}

/// Build opaque cursor from last row key (stable sort key).
pub fn encode_cursor(key: &str) -> String {
    key.to_string()
}

pub fn decode_cursor(cursor: &str) -> Option<String> {
    if cursor.is_empty() {
        None
    } else {
        Some(cursor.to_string())
    }
}

/// Close a JSON page when the next row would exceed `PAGE_BYTE_CAP`; set `next_cursor`.
pub fn page_close_on_byte_budget<T: Serialize>(
    rows: &mut Vec<T>,
    sort_keys: &mut Vec<String>,
    serializer: impl Fn(&T) -> serde_json::Value,
) -> Option<String> {
    let mut size = 2; // `[]`
    let mut keep = 0;
    for (row, key) in rows.iter().zip(sort_keys.iter()) {
        let json = serde_json::to_vec(&serializer(row)).unwrap_or_default();
        let next = size + json.len() + if keep > 0 { 1 } else { 0 };
        if next > PAGE_BYTE_CAP {
            break;
        }
        size = next;
        keep += 1;
    }
    if keep < rows.len() {
        let next_key = sort_keys.get(keep).cloned();
        rows.truncate(keep);
        sort_keys.truncate(keep);
        return next_key.map(|k| encode_cursor(&k));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limit_default_and_cap() {
        assert_eq!(parse_limit(None).unwrap(), 50);
        assert_eq!(parse_limit(Some("100")).unwrap(), 100);
        assert!(parse_limit(Some("101")).is_err());
        assert!(parse_limit(Some("0")).is_err());
    }

    #[test]
    fn cursor_roundtrip() {
        let c = encode_cursor("acme");
        assert_eq!(decode_cursor(&c).as_deref(), Some("acme"));
    }
}
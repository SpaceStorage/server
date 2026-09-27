//! L4 compositions — federated / union / materialized_view at planetary scale (003 / 004 / intent 16).

use crate::descriptor::{Kind, Level, TypeDescriptor};
use crate::error::TypeError;
use crate::ident::{new_container_id, ContainerId, ContainerName, NamespaceName};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const L4_COMPOSITIONS: &[&str] = &[
    "union",
    "federated",
    "materialized_view",
    "distributed",
    "partitioned",
    "replicated",
    "sharded",
];

pub fn l4_descriptor(name: &str) -> Option<TypeDescriptor> {
    if !L4_COMPOSITIONS.contains(&name) {
        return None;
    }
    Some(TypeDescriptor {
        name: name.into(),
        display_name: match name {
            "union" => "Union",
            "federated" => "Federated",
            "materialized_view" => "Materialized View",
            "distributed" => "Distributed",
            "partitioned" => "Partitioned",
            "replicated" => "Replicated",
            "sharded" => "Sharded",
            _ => name,
        }
        .into(),
        level: Level::L4,
        kind: Kind::Composition,
        multi_active: false,
        schema_required: false,
    })
}

/// Placement hint for planetary federated / union / MV surfaces (`planet` is a label).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PlanetaryPlacement {
    /// Topology ladder labels, e.g. `planet=earth`, `region=eu`, `az=a`.
    pub labels: HashMap<String, String>,
    /// Explicit `quorum_domain` for local durable quorum (004).
    pub quorum_domain: String,
}

impl PlanetaryPlacement {
    pub fn planet(&self) -> Option<&str> {
        self.labels.get("planet").map(|s| s.as_str())
    }

    pub fn region(&self) -> Option<&str> {
        self.labels.get("region").map(|s| s.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompositionKind {
    Union,
    Federated,
    MaterializedView,
    Distributed,
    Partitioned,
    Replicated,
    Sharded,
}

impl CompositionKind {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "union" => Some(Self::Union),
            "federated" => Some(Self::Federated),
            "materialized_view" => Some(Self::MaterializedView),
            "distributed" => Some(Self::Distributed),
            "partitioned" => Some(Self::Partitioned),
            "replicated" => Some(Self::Replicated),
            "sharded" => Some(Self::Sharded),
            _ => None,
        }
    }

    pub fn machine_name(self) -> &'static str {
        match self {
            Self::Union => "union",
            Self::Federated => "federated",
            Self::MaterializedView => "materialized_view",
            Self::Distributed => "distributed",
            Self::Partitioned => "partitioned",
            Self::Replicated => "replicated",
            Self::Sharded => "sharded",
        }
    }

    pub fn writes_allowed(self) -> bool {
        !matches!(self, Self::Union | Self::MaterializedView)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositionMember {
    pub namespace: String,
    pub container: String,
    /// Optional routing key / predicate fragment for federated.
    #[serde(default)]
    pub route_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Composition {
    pub id: ContainerId,
    pub name: ContainerName,
    pub namespace: NamespaceName,
    pub kind: CompositionKind,
    pub members: Vec<CompositionMember>,
    pub placement: PlanetaryPlacement,
    /// Last refresh stamp for materialized views (unix millis).
    pub refreshed_at_ms: Option<u64>,
    /// Materialized snapshot (key → value).
    materialized: HashMap<String, Vec<u8>>,
}

impl Composition {
    pub fn create(
        namespace: impl Into<String>,
        name: impl Into<String>,
        kind: CompositionKind,
        members: Vec<CompositionMember>,
        placement: PlanetaryPlacement,
    ) -> Result<Self, TypeError> {
        if members.is_empty() {
            return Err(TypeError::Msg("composition requires members".into()));
        }
        if matches!(kind, CompositionKind::Union | CompositionKind::Federated | CompositionKind::Sharded)
            && members.len() < 2
        {
            return Err(TypeError::Msg(format!(
                "{} requires ≥ 2 members",
                kind.machine_name()
            )));
        }
        // Cross-namespace refuse for union (FR depth / namespace rules simplified).
        if kind == CompositionKind::Union {
            let ns0 = &members[0].namespace;
            if members.iter().any(|m| m.namespace != *ns0) {
                return Err(TypeError::Msg(
                    "union members must share namespace; use federated for cross-namespace"
                        .into(),
                ));
            }
        }
        if placement.quorum_domain.is_empty() {
            return Err(TypeError::Msg("quorum_domain required".into()));
        }
        Ok(Self {
            id: new_container_id(),
            name: ContainerName::new(name.into()),
            namespace: NamespaceName::new(namespace.into()),
            kind,
            members,
            placement,
            refreshed_at_ms: None,
            materialized: HashMap::new(),
        })
    }

    /// Route a federated write: single member whose `route_key` matches (or sole member).
    pub fn route_write(&self, route: &str) -> Result<&CompositionMember, TypeError> {
        if !self.kind.writes_allowed() {
            return Err(TypeError::Msg(format!(
                "{} is read-only",
                self.kind.machine_name()
            )));
        }
        if self.kind != CompositionKind::Federated {
            return self.members.first().ok_or(TypeError::NotFound);
        }
        self.members
            .iter()
            .find(|m| m.route_key.as_deref() == Some(route))
            .or_else(|| {
                if self.members.len() == 1 {
                    self.members.first()
                } else {
                    None
                }
            })
            .ok_or_else(|| TypeError::Msg("federated write ambiguous".into()))
    }

    pub fn refresh_materialized(
        &mut self,
        rows: HashMap<String, Vec<u8>>,
        now_ms: u64,
    ) -> Result<(), TypeError> {
        if self.kind != CompositionKind::MaterializedView {
            return Err(TypeError::Msg("not a materialized_view".into()));
        }
        self.materialized = rows;
        self.refreshed_at_ms = Some(now_ms);
        Ok(())
    }

    pub fn read_materialized(&self, key: &str) -> Result<Option<&[u8]>, TypeError> {
        if self.kind != CompositionKind::MaterializedView {
            return Err(TypeError::Msg("not a materialized_view".into()));
        }
        Ok(self.materialized.get(key).map(|v| v.as_slice()))
    }

    pub fn is_planetary(&self) -> bool {
        self.placement.planet().is_some()
            || self
                .placement
                .labels
                .keys()
                .any(|k| k == "continent" || k == "region")
    }
}

#[derive(Debug, Default)]
pub struct CompositionCatalog {
    items: HashMap<(String, String), Composition>,
}

impl CompositionCatalog {
    pub fn create(
        &mut self,
        namespace: impl Into<String>,
        name: impl Into<String>,
        kind: CompositionKind,
        members: Vec<CompositionMember>,
        placement: PlanetaryPlacement,
    ) -> Result<ContainerId, TypeError> {
        let ns = namespace.into();
        let n = name.into();
        let key = (ns.clone(), n.clone());
        if self.items.contains_key(&key) {
            return Err(TypeError::AlreadyExists);
        }
        let c = Composition::create(ns, n, kind, members, placement)?;
        let id = c.id;
        self.items.insert(key, c);
        Ok(id)
    }

    pub fn get(&self, namespace: &str, name: &str) -> Result<&Composition, TypeError> {
        self.items
            .get(&(namespace.into(), name.into()))
            .ok_or(TypeError::NotFound)
    }

    pub fn get_mut(&mut self, namespace: &str, name: &str) -> Result<&mut Composition, TypeError> {
        self.items
            .get_mut(&(namespace.into(), name.into()))
            .ok_or(TypeError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planetary_federated_routes() {
        let mut cat = CompositionCatalog::default();
        let mut labels = HashMap::new();
        labels.insert("planet".into(), "earth".into());
        labels.insert("region".into(), "eu".into());
        let id = cat
            .create(
                "acme",
                "fed",
                CompositionKind::Federated,
                vec![
                    CompositionMember {
                        namespace: "acme".into(),
                        container: "eu_kv".into(),
                        route_key: Some("eu".into()),
                    },
                    CompositionMember {
                        namespace: "acme".into(),
                        container: "us_kv".into(),
                        route_key: Some("us".into()),
                    },
                ],
                PlanetaryPlacement {
                    labels,
                    quorum_domain: "eu-1".into(),
                },
            )
            .unwrap();
        let c = cat.get("acme", "fed").unwrap();
        assert_eq!(c.id, id);
        assert!(c.is_planetary());
        assert_eq!(c.route_write("eu").unwrap().container, "eu_kv");
        assert!(c.route_write("xx").is_err());
    }

    #[test]
    fn union_readonly_and_mv_refresh() {
        let mut cat = CompositionCatalog::default();
        cat.create(
            "acme",
            "u",
            CompositionKind::Union,
            vec![
                CompositionMember {
                    namespace: "acme".into(),
                    container: "a".into(),
                    route_key: None,
                },
                CompositionMember {
                    namespace: "acme".into(),
                    container: "b".into(),
                    route_key: None,
                },
            ],
            PlanetaryPlacement {
                labels: HashMap::new(),
                quorum_domain: "local".into(),
            },
        )
        .unwrap();
        assert!(cat.get("acme", "u").unwrap().route_write("x").is_err());

        cat.create(
            "acme",
            "mv",
            CompositionKind::MaterializedView,
            vec![CompositionMember {
                namespace: "acme".into(),
                container: "src".into(),
                route_key: None,
            }],
            PlanetaryPlacement {
                labels: [("planet".into(), "earth".into())].into(),
                quorum_domain: "local".into(),
            },
        )
        .unwrap();
        let mv = cat.get_mut("acme", "mv").unwrap();
        mv.refresh_materialized(
            [("k".into(), b"v".to_vec())].into(),
            1_000,
        )
        .unwrap();
        assert_eq!(mv.read_materialized("k").unwrap(), Some(b"v".as_slice()));
    }
}

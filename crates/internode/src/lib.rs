//! Internode + replication always-on ports; default quorum_domain (012 MVP).

#[derive(Debug, Clone)]
pub struct FabricConfig {
    pub internode_bound: bool,
    pub replication_bound: bool,
    pub quorum_domain: String,
}

impl FabricConfig {
    pub fn require_always_on(internode: bool, replication: bool, domain: impl Into<String>) -> Result<Self, &'static str> {
        if !internode {
            return Err("internode_required");
        }
        if !replication {
            return Err("replication_required");
        }
        Ok(Self {
            internode_bound: true,
            replication_bound: true,
            quorum_domain: domain.into(),
        })
    }
}

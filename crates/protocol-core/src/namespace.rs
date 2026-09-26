//! Namespace binding helpers (FR-009a) — stub until full auth seam lands.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamespaceSource {
    ClientSelected,
    CredentialBound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundNamespace {
    pub name: String,
    pub source: NamespaceSource,
}

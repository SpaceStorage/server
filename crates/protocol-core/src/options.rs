//! Shared option literal parsing placeholders (US3 deepens this).

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionOptionBag {
    pub write_quorum: Option<String>,
    pub read_quorum: Option<String>,
    pub timeout: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageMode {
    Memory,
    Persistent,
    Hybrid,
}

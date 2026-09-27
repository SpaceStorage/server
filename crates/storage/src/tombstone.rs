//! Tombstone / gc_grace seam (US3).
//!
//! See [`crate::legal_hold`] for GDPR erase + legal-hold product workflows that
//! emit tombstones when holds are clear.

/// Placeholder for LSM tombstone metrics / GC grace; erase records live in legal_hold.
pub fn gc_grace_default_secs() -> u64 {
    86400
}
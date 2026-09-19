//! Conformance harness seams for the first-binary profile.
//!
//! Behavior is supplied by sibling crates (`001`–`015`). This crate owns the
//! gate tests under `--features first-binary`.

use spacestorage_release_profile::{HandlerBuildSet, ReleaseProfile, TypeRequirement};

/// Compile-time / status hint for the default build.
pub const RELEASE_PROFILE: &str = "first-binary";

pub fn first_binary_handlers() -> HandlerBuildSet {
    ReleaseProfile::FirstBinary.handler_set()
}

pub fn first_binary_types() -> TypeRequirement {
    ReleaseProfile::FirstBinary.type_requirement()
}

/// Placeholder until `crates/node` boots in-process nodes for G1–G11.
pub fn in_process_cluster_ready() -> bool {
    false
}

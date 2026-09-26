//! SpaceStorage release profile: slices, handlers, ledger, product non-goals.
//!
//! This crate selects and proves the slices 1–5 subset. It does not implement
//! protocol handlers or a second query engine.

pub mod error;
pub mod handlers;
pub mod ledger;
pub mod nongoals;
pub mod profile;
pub mod slice;
pub mod types;

pub use error::ValidationCode;
pub use handlers::HandlerBuildSet;
pub use ledger::{DeferredSlice, MilestoneRecord};
pub use nongoals::{ProductNonGoal, REQUIRED_MENTIONS, audit_nongoals};
pub use profile::ReleaseProfile;
pub use slice::{DEFERRED_AFTER_FIRST, FIRST_BINARY, SliceId, validate_implemented_prefix};
pub use types::TypeRequirement;

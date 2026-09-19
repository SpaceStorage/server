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
pub use nongoals::{audit_nongoals, ProductNonGoal, REQUIRED_MENTIONS};
pub use profile::ReleaseProfile;
pub use slice::{
    validate_implemented_prefix, SliceId, DEFERRED_AFTER_FIRST, FIRST_BINARY,
};
pub use types::TypeRequirement;

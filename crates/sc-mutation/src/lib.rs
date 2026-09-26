//! The mutation runner lives in `sc-engines`.
//!
//! This crate remains so the workspace layout stays in place.

/// Mutation runs through `sc-engines` when `--mutation` is not `off`.
pub fn implemented() -> bool {
    true
}

//! The mutation runner lives in `ca-engines`.
//!
//! This crate remains so the workspace layout stays in place.

/// Mutation runs through `ca-engines` when `--mutation` is not `off`.
pub fn implemented() -> bool {
    true
}

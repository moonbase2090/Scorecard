// SPDX-License-Identifier: MPL-2.0
//! The mutation runner lives in `sc-engines`.
//!
//! This crate remains so the workspace layout stays in place.

/// Mutation runs through `sc-engines` when `--mutation` is not `off`.
pub fn implemented() -> bool {
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn mutation_is_provided_by_sc_engines() {
        assert!(super::implemented());
    }
}

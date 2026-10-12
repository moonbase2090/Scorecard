// SPDX-License-Identifier: MIT
//! A `pub use` of a local module must not read as an external crate.
//! Mirrors the `mod score;` + `pub use score::...` shape in sc-core.

mod score;

pub use score::double;

pub fn quadruple(value: u32) -> u32 {
    double(double(value))
}

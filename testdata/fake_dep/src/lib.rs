// SPDX-License-Identifier: MPL-2.0
/// The import is cfg-off so `cargo check` still passes.
/// `sc` still sees `missing_crate` and reports `sca.undeclared_dependency`.
#[cfg(any())]
use missing_crate::Thing;

pub fn reserved() -> u32 {
    let _hidden = std::any::type_name::<u32>();
    1
}

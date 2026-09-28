// SPDX-License-Identifier: MPL-2.0
use syn::File;

use sc_core::Span;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerfHit {
    pub file: String,
    pub symbol: String,
    pub rule: String,
    pub message: String,
    pub span: Span,
}

/// A nested loop, and a `.clone()` that the loop collects, are ordinary Rust.
/// They are not findings.
pub fn perf_in_file(_file: &File, _rel: &str) -> Vec<PerfHit> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_owned_row_clone_is_not_a_finding() {
        let src = r#"
pub fn owned_rows(rows: &[Vec<String>]) -> Vec<String> {
    let mut out = Vec::new();
    for row in rows {
        for cell in row {
            out.push(cell.clone());
        }
    }
    out
}
"#;
        let file = syn::parse_file(src).unwrap();
        assert!(perf_in_file(&file, "src/lib.rs").is_empty());
    }

    #[test]
    fn impl_trait_and_module_functions_are_not_findings() {
        let src = r#"
impl Foo {
    fn m(&self) { let _ = 1; }
}
trait Bar {
    fn go() { let _ = 1; }
}
mod inner {
    pub fn hidden() { let _ = 1; }
}
"#;
        let file = syn::parse_file(src).unwrap();
        assert!(perf_in_file(&file, "src/lib.rs").is_empty());
    }
}

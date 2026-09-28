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
/// They are not findings. Test modules are not scanned: a later rule must not
/// annotate `tests/`, `test/`, or a test file name.
pub fn perf_in_file(_file: &File, rel: &str) -> Vec<PerfHit> {
    if test_module(rel) {
        return Vec::new();
    }
    Vec::new()
}

fn test_module(rel: &str) -> bool {
    let rel = rel.trim_start_matches("./").replace('\\', "/");
    if rel.split('/').any(|part| {
        matches!(part, "test" | "tests" | "__tests__" | "spec" | "testdata")
            || part.ends_with(".Tests")
            || part.ends_with(".Test")
    }) {
        return true;
    }
    let name = rel.rsplit('/').next().unwrap_or("");
    if name == "conftest.py" {
        return true;
    }
    let stem = name.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(name);
    stem.starts_with("test_")
        || [
            "_test",
            "_tests",
            "_spec",
            "_unittest",
            "-test",
            "-spec",
            ".test",
            ".spec",
            "Test",
            "Tests",
        ]
        .iter()
        .any(|suffix| stem.ends_with(suffix))
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

    #[test]
    fn test_modules_are_not_scanned() {
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
        assert!(test_module("tests/rows.rs"));
        assert!(test_module("src/rows_test.rs"));
        assert!(test_module("test/rows.rs"));
        assert!(!test_module("src/lib.rs"));
        assert!(perf_in_file(&file, "tests/rows.rs").is_empty());
        assert!(perf_in_file(&file, "src/rows_test.rs").is_empty());
        assert!(perf_in_file(&file, "src/lib.rs").is_empty());
    }
}

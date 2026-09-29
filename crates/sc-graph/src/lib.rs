// SPDX-License-Identifier: MPL-2.0
//! Source discovery and cyclomatic complexity.
//!
//! Complexity is McCabe cyclomatic complexity, not cognitive complexity.
//! Macros are not expanded. Nested closures count toward the enclosing function.
//! Nested `fn` items are not folded into the parent.

mod complexity;
mod discover;
mod imports;
mod perf;
mod pubs;

pub use complexity::{cyclomatic_of_block, functions_in_source, FunctionInfo};

pub fn function_symbols(text: &str, rel: &str) -> Vec<String> {
    let Ok(file) = syn::parse_file(text) else {
        return Vec::new();
    };
    functions_in_source(&file, rel)
        .into_iter()
        .map(|function| function.symbol)
        .collect()
}

pub struct SourceFacts {
    pub functions: Vec<FunctionInfo>,
    pub imports: Vec<ImportHit>,
    pub perf: Vec<PerfHit>,
    pub items: Vec<PubItem>,
    /// `mod` declarations and `extern crate` rename targets in this file.
    pub local_names: Vec<String>,
}

pub fn inspect_source(text: &str, rel: &str, perf_enabled: bool) -> Option<SourceFacts> {
    let file = syn::parse_file(text).ok()?;
    Some(SourceFacts {
        functions: functions_in_source(&file, rel),
        imports: imports_in_file(&file, rel),
        perf: perf_in_file(&file, rel, perf_enabled),
        items: pub_items_in_file(&file, rel),
        local_names: local_names_in_file(&file),
    })
}
pub use discover::{is_excluded, scan, source_files, source_files_under, Scan};
pub use imports::{imports_in_file, local_names_in_file, ImportHit};
pub use perf::{
    cfg_test_coverage, is_cfg_test_only, module_subtree_prefix, out_of_line_cfg_test_paths,
    out_of_line_cfg_test_paths_from_source, perf_in_file, PerfHit,
};
pub use pubs::{pub_items_in_file, PubItem};

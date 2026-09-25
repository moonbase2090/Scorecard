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
}

pub fn inspect_source(text: &str, rel: &str) -> Option<SourceFacts> {
    let file = syn::parse_file(text).ok()?;
    Some(SourceFacts {
        functions: functions_in_source(&file, rel),
        imports: imports_in_file(&file, rel),
        perf: perf_in_file(&file, rel),
        items: pub_items_in_file(&file, rel),
    })
}
pub use discover::{is_excluded, scan, source_files, Scan};
pub use imports::{imports_in_file, ImportHit};
pub use perf::{perf_in_file, PerfHit};
pub use pubs::{pub_items_in_file, PubItem};

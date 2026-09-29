// SPDX-License-Identifier: MPL-2.0
//! Performance hints.
//!
//! Off by default (`engines.perf = false`). When enabled, a nested loop and a
//! `.clone()` inside a loop are `perf.*` findings (disposition `ignore`).
//! Paths under `tests/` or `benches/`, and `#[cfg(test)]` / `#[…::test]` items,
//! are not scanned.

use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{Expr, File, ImplItem, Item, TraitItem};

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

/// Scan a file for performance hits when `enabled`. Test paths and test items
/// are skipped. When `enabled` is false, returns empty without walking the AST.
pub fn perf_in_file(file: &File, rel: &str, enabled: bool) -> Vec<PerfHit> {
    if !enabled || is_test_path(rel) {
        return Vec::new();
    }
    let mut out = Vec::new();
    walk_items(&file.items, rel, "", &mut out);
    out
}

/// Relative paths of files that an out-of-line `#[cfg(test)] mod name;` (or
/// `#[cfg(test)] #[path = "..."] mod name;`) in `declaring_rel` would load.
/// Candidates are both `name.rs` and `name/mod.rs` when there is no `#[path]`.
pub fn out_of_line_cfg_test_paths(file: &File, declaring_rel: &str) -> Vec<String> {
    let mut out = Vec::new();
    collect_out_of_line_cfg_test(&file.items, declaring_rel, &mut out);
    out
}

/// Parse `text` and return out-of-line `#[cfg(test)]` module paths.
pub fn out_of_line_cfg_test_paths_from_source(text: &str, declaring_rel: &str) -> Vec<String> {
    let Ok(file) = syn::parse_file(text) else {
        return Vec::new();
    };
    out_of_line_cfg_test_paths(&file, declaring_rel)
}

fn collect_out_of_line_cfg_test(items: &[Item], declaring_rel: &str, out: &mut Vec<String>) {
    for item in items {
        let Item::Mod(module) = item else {
            continue;
        };
        if let Some((_, nested)) = &module.content {
            // Inline module: recurse so a nested out-of-line child is found.
            // Children of an inline mod live beside the declaring file's children dir.
            let child_decl = match module_children_dir(declaring_rel) {
                dir if dir.is_empty() => format!("{}.rs", module.ident),
                dir => format!("{dir}/{}.rs", module.ident),
            };
            collect_out_of_line_cfg_test(nested, &child_decl, out);
            continue;
        }
        if !is_cfg_test(&module.attrs) {
            continue;
        }
        if let Some(path) = path_attr(&module.attrs) {
            out.push(join_rel(&parent_dir(declaring_rel), &path));
            continue;
        }
        let dir = module_children_dir(declaring_rel);
        let name = module.ident.to_string();
        out.push(join_rel(&dir, &format!("{name}.rs")));
        out.push(join_rel(&dir, &format!("{name}/mod.rs")));
    }
}

fn path_attr(attrs: &[syn::Attribute]) -> Option<String> {
    for attr in attrs {
        if !attr.path().is_ident("path") {
            continue;
        }
        match &attr.meta {
            syn::Meta::NameValue(nv) => {
                if let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(lit),
                    ..
                }) = &nv.value
                {
                    return Some(lit.value().replace('\\', "/"));
                }
            }
            syn::Meta::List(_) => {
                if let Ok(lit) = attr.parse_args::<syn::LitStr>() {
                    return Some(lit.value().replace('\\', "/"));
                }
            }
            _ => {}
        }
    }
    None
}

fn normalize_rel(rel: &str) -> String {
    rel.trim_start_matches("./").replace('\\', "/")
}

fn parent_dir(rel: &str) -> String {
    let rel = normalize_rel(rel);
    match rel.rsplit_once('/') {
        Some((parent, _)) => parent.to_string(),
        None => String::new(),
    }
}

/// Directory that holds submodules of `declaring_rel` (Rust module layout).
fn module_children_dir(declaring_rel: &str) -> String {
    let rel = normalize_rel(declaring_rel);
    let file_name = rel.rsplit('/').next().unwrap_or(rel.as_str());
    let parent = parent_dir(&rel);
    if matches!(file_name, "lib.rs" | "main.rs" | "mod.rs") {
        return parent;
    }
    let stem = file_name
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(file_name);
    join_rel(&parent, stem)
}

fn join_rel(dir: &str, name: &str) -> String {
    let name = name.trim_start_matches("./");
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

/// Rust integration-test and bench trees, plus `src/test.rs` / `src/tests.rs`.
/// Product code under `src/spec/` stays scanned.
pub(crate) fn is_test_path(rel: &str) -> bool {
    let rel = normalize_rel(rel);
    if rel
        .split('/')
        .any(|part| matches!(part, "tests" | "benches"))
    {
        return true;
    }
    // Cheap guard for the common out-of-line `#[cfg(test)] mod tests;` file.
    let name = rel.rsplit('/').next().unwrap_or("");
    let stem = name.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(name);
    matches!(stem, "test" | "tests") && rel.split('/').any(|part| part == "src")
}

fn is_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("cfg") {
            return false;
        }
        matches!(attr.parse_args::<syn::Ident>(), Ok(ident) if ident == "test")
    })
}

fn is_test_attr(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path()
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "test")
    })
}

fn skip_item(attrs: &[syn::Attribute]) -> bool {
    is_cfg_test(attrs) || is_test_attr(attrs)
}

#[inline(never)]
fn walk_items(items: &[Item], file: &str, prefix: &str, out: &mut Vec<PerfHit>) {
    for item in items {
        match item {
            Item::Fn(func) => {
                if skip_item(&func.attrs) {
                    continue;
                }
                let symbol = qualify(prefix, &func.sig.ident.to_string());
                scan_block(file, &symbol, &func.block, out);
            }
            Item::Impl(imp) => {
                if is_cfg_test(&imp.attrs) {
                    continue;
                }
                let ty = imp
                    .self_ty
                    .span()
                    .source_text()
                    .unwrap_or_else(|| "Self".into());
                let ty = ty.split('<').next().unwrap_or("Self").trim();
                let impl_prefix = qualify(prefix, ty);
                for item in &imp.items {
                    if let ImplItem::Fn(method) = item {
                        if skip_item(&method.attrs) {
                            continue;
                        }
                        let symbol = qualify(&impl_prefix, &method.sig.ident.to_string());
                        scan_block(file, &symbol, &method.block, out);
                    }
                }
            }
            Item::Trait(trait_item) => {
                if is_cfg_test(&trait_item.attrs) {
                    continue;
                }
                let trait_prefix = qualify(prefix, &trait_item.ident.to_string());
                for item in &trait_item.items {
                    if let TraitItem::Fn(method) = item {
                        if let Some(block) = &method.default {
                            if skip_item(&method.attrs) {
                                continue;
                            }
                            let symbol = qualify(&trait_prefix, &method.sig.ident.to_string());
                            scan_block(file, &symbol, block, out);
                        }
                    }
                }
            }
            Item::Mod(module) => {
                if is_cfg_test(&module.attrs) {
                    continue;
                }
                if let Some((_, items)) = &module.content {
                    let next = qualify(prefix, &module.ident.to_string());
                    walk_items(items, file, &next, out);
                }
            }
            _ => {}
        }
    }
}

fn qualify(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}::{name}")
    }
}

fn scan_block(file: &str, symbol: &str, block: &syn::Block, out: &mut Vec<PerfHit>) {
    let mut visitor = PerfVisitor {
        file,
        symbol,
        loop_depth: 0,
        out,
    };
    visitor.visit_block(block);
}

struct PerfVisitor<'a> {
    file: &'a str,
    symbol: &'a str,
    loop_depth: u32,
    out: &'a mut Vec<PerfHit>,
}

impl PerfVisitor<'_> {
    fn span_of(&self, span: proc_macro2::Span) -> Span {
        let start = span.start();
        let end = span.end();
        Span {
            start_line: start.line as u32,
            start_col: start.column as u32 + 1,
            end_line: end.line as u32,
            end_col: end.column as u32 + 1,
        }
    }

    fn push(&mut self, rule: &str, message: String, span: proc_macro2::Span) {
        self.out.push(PerfHit {
            file: self.file.to_string(),
            symbol: self.symbol.to_string(),
            rule: rule.to_string(),
            message,
            span: self.span_of(span),
        });
    }
}

impl<'ast> Visit<'ast> for PerfVisitor<'_> {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        match expr {
            Expr::ForLoop(expr_loop) => {
                if self.loop_depth >= 1 {
                    self.push(
                        &format!("perf.{}", "nested_loop"),
                        format!("nested loop in {}", self.symbol),
                        expr_loop.for_token.span(),
                    );
                }
                self.loop_depth += 1;
                syn::visit::visit_expr(self, expr);
                self.loop_depth -= 1;
            }
            Expr::While(expr_loop) => {
                if self.loop_depth >= 1 {
                    self.push(
                        &format!("perf.{}", "nested_loop"),
                        format!("nested loop in {}", self.symbol),
                        expr_loop.while_token.span(),
                    );
                }
                self.loop_depth += 1;
                syn::visit::visit_expr(self, expr);
                self.loop_depth -= 1;
            }
            Expr::Loop(expr_loop) => {
                if self.loop_depth >= 1 {
                    self.push(
                        &format!("perf.{}", "nested_loop"),
                        format!("nested loop in {}", self.symbol),
                        expr_loop.loop_token.span(),
                    );
                }
                self.loop_depth += 1;
                syn::visit::visit_expr(self, expr);
                self.loop_depth -= 1;
            }
            Expr::MethodCall(call) if call.method == "clone" && self.loop_depth > 0 => {
                self.push(
                    &format!("perf.{}", "clone_in_loop"),
                    format!("clone inside a loop in {}", self.symbol),
                    call.method.span(),
                );
                syn::visit::visit_expr(self, expr);
            }
            _ => syn::visit::visit_expr(self, expr),
        }
    }

    fn visit_item(&mut self, _item: &'ast Item) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLONE_SRC: &str = r#"
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

    #[test]
    fn disabled_emits_nothing_and_does_not_need_a_walk_result() {
        let file = syn::parse_file(CLONE_SRC).unwrap();
        assert!(perf_in_file(&file, "src/lib.rs", false).is_empty());
    }

    #[test]
    fn enabled_flags_nested_loop_and_clone() {
        let file = syn::parse_file(CLONE_SRC).unwrap();
        let hits = perf_in_file(&file, "src/lib.rs", true);
        let rules: Vec<_> = hits.iter().map(|hit| hit.rule.as_str()).collect();
        assert!(rules.contains(&"perf.nested_loop"), "{rules:?}");
        assert!(rules.contains(&"perf.clone_in_loop"), "{rules:?}");
    }

    #[test]
    fn an_owned_row_clone_is_not_a_finding_by_default() {
        let file = syn::parse_file(CLONE_SRC).unwrap();
        assert!(perf_in_file(&file, "src/lib.rs", false).is_empty());
    }

    #[test]
    fn impl_trait_and_module_functions_with_no_pattern_are_clean() {
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
        assert!(perf_in_file(&file, "src/lib.rs", true).is_empty());
    }

    #[test]
    fn test_paths_are_not_scanned() {
        assert!(is_test_path("tests/rows.rs"));
        assert!(is_test_path("crates/foo/tests/it.rs"));
        assert!(is_test_path("benches/hot.rs"));
        assert!(is_test_path("src/tests.rs"));
        assert!(is_test_path("src/test.rs"));
        assert!(is_test_path("crates/foo/src/tests.rs"));
        assert!(!is_test_path("src/lib.rs"));
        assert!(!is_test_path("src/spec/mod.rs"));
        assert!(!is_test_path("src/rows_test.rs"));
        assert!(!is_test_path("src/helpers.rs"));

        let file = syn::parse_file(CLONE_SRC).unwrap();
        assert!(
            !perf_in_file(&file, "src/lib.rs", true).is_empty(),
            "enabled scan must see the clone so the skip is observable"
        );
        assert!(perf_in_file(&file, "tests/rows.rs", true).is_empty());
        assert!(perf_in_file(&file, "benches/hot.rs", true).is_empty());
        assert!(perf_in_file(&file, "src/tests.rs", true).is_empty());
    }

    #[test]
    fn out_of_line_cfg_test_mod_resolves_beside_lib() {
        let lib = syn::parse_file(
            r#"
pub fn product() {}
#[cfg(test)]
mod helpers;
"#,
        )
        .unwrap();
        let paths = out_of_line_cfg_test_paths(&lib, "src/lib.rs");
        assert!(paths.contains(&"src/helpers.rs".into()), "{paths:?}");
        assert!(paths.contains(&"src/helpers/mod.rs".into()), "{paths:?}");

        let with_path = syn::parse_file(
            r#"
#[cfg(test)]
#[path = "alt/probe.rs"]
mod helpers;
"#,
        )
        .unwrap();
        assert_eq!(
            out_of_line_cfg_test_paths(&with_path, "src/lib.rs"),
            vec!["src/alt/probe.rs".to_string()]
        );

        // File module: children live under src/foo/
        let foo = syn::parse_file("#[cfg(test)]\nmod helpers;\n").unwrap();
        let paths = out_of_line_cfg_test_paths(&foo, "src/foo.rs");
        assert!(paths.contains(&"src/foo/helpers.rs".into()), "{paths:?}");
    }

    #[test]
    fn cfg_test_modules_are_not_scanned() {
        let src = r#"
pub fn product(rows: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for row in rows {
        out.push(row.clone());
    }
    out
}

#[cfg(test)]
mod tests {
    pub fn helper(rows: &[String]) -> Vec<String> {
        let mut out = Vec::new();
        for row in rows {
            out.push(row.clone());
        }
        out
    }
}
"#;
        let file = syn::parse_file(src).unwrap();
        let hits = perf_in_file(&file, "src/lib.rs", true);
        assert!(
            hits.iter().any(|hit| hit.symbol == "product"),
            "product clone should be visible when enabled: {hits:?}"
        );
        assert!(
            hits.iter().all(|hit| hit.symbol != "tests::helper"),
            "cfg(test) helper must be skipped: {hits:?}"
        );
        assert!(perf_in_file(&file, "src/lib.rs", false).is_empty());
    }

    #[test]
    fn tokio_test_functions_are_not_scanned() {
        let src = r#"
pub fn product(rows: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for row in rows {
        out.push(row.clone());
    }
    out
}

#[tokio::test]
async fn helper() {
    let rows = vec!["a".to_string()];
    for row in &rows {
        let _ = row.clone();
    }
}
"#;
        let file = syn::parse_file(src).unwrap();
        let hits = perf_in_file(&file, "src/lib.rs", true);
        assert!(hits.iter().any(|hit| hit.symbol == "product"), "{hits:?}");
        assert!(
            hits.iter().all(|hit| hit.symbol != "helper"),
            "#[tokio::test] must be skipped: {hits:?}"
        );
    }
}

// SPDX-License-Identifier: MPL-2.0
//! Performance hints.
//!
//! A nested loop, and a `.clone()` that the loop collects, are ordinary Rust
//! and are not findings. Test modules are not scanned: a later rule must not
//! annotate `tests/`, a `*_test.rs` file, or a `#[cfg(test)]` module.

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

#[derive(Clone, Copy)]
struct Rules {
    nested_loop: bool,
    clone_in_loop: bool,
}

/// Production rules stay off: nested loops and collecting clones are not findings.
const PRODUCTION: Rules = Rules {
    nested_loop: false,
    clone_in_loop: false,
};

/// Scan a file for performance hits. Test paths are skipped entirely.
pub fn perf_in_file(file: &File, rel: &str) -> Vec<PerfHit> {
    if is_test_path(rel) {
        return Vec::new();
    }
    scan(file, rel, PRODUCTION)
}

fn scan(file: &File, rel: &str, rules: Rules) -> Vec<PerfHit> {
    let mut out = Vec::new();
    walk_items(&file.items, rel, "", rules, &mut out);
    out
}

/// Path looks like test code (same shapes CRAP skips in the other packs).
pub(crate) fn is_test_path(rel: &str) -> bool {
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

fn is_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("cfg") {
            return false;
        }
        matches!(attr.parse_args::<syn::Ident>(), Ok(ident) if ident == "test")
    })
}

#[inline(never)]
fn walk_items(items: &[Item], file: &str, prefix: &str, rules: Rules, out: &mut Vec<PerfHit>) {
    for item in items {
        match item {
            Item::Fn(func) => {
                if is_cfg_test(&func.attrs)
                    || func.attrs.iter().any(|attr| attr.path().is_ident("test"))
                {
                    continue;
                }
                let symbol = qualify(prefix, &func.sig.ident.to_string());
                scan_block(file, &symbol, &func.block, rules, out);
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
                        if is_cfg_test(&method.attrs)
                            || method.attrs.iter().any(|attr| attr.path().is_ident("test"))
                        {
                            continue;
                        }
                        let symbol = qualify(&impl_prefix, &method.sig.ident.to_string());
                        scan_block(file, &symbol, &method.block, rules, out);
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
                            if is_cfg_test(&method.attrs) {
                                continue;
                            }
                            let symbol = qualify(&trait_prefix, &method.sig.ident.to_string());
                            scan_block(file, &symbol, block, rules, out);
                        }
                    }
                }
            }
            Item::Mod(module) => {
                // Test modules are not scanned.
                if is_cfg_test(&module.attrs) {
                    continue;
                }
                if let Some((_, items)) = &module.content {
                    let next = qualify(prefix, &module.ident.to_string());
                    walk_items(items, file, &next, rules, out);
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

fn scan_block(file: &str, symbol: &str, block: &syn::Block, rules: Rules, out: &mut Vec<PerfHit>) {
    let mut visitor = PerfVisitor {
        file,
        symbol,
        loop_depth: 0,
        rules,
        out,
    };
    visitor.visit_block(block);
}

struct PerfVisitor<'a> {
    file: &'a str,
    symbol: &'a str,
    loop_depth: u32,
    rules: Rules,
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
                if self.rules.nested_loop && self.loop_depth >= 1 {
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
                if self.rules.nested_loop && self.loop_depth >= 1 {
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
                if self.rules.nested_loop && self.loop_depth >= 1 {
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
                if self.rules.clone_in_loop {
                    self.push(
                        &format!("perf.{}", "clone_in_loop"),
                        format!("clone inside a loop in {}", self.symbol),
                        call.method.span(),
                    );
                }
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

    /// Probe rules that exist only so tests can prove the skip gates.
    const PROBE: Rules = Rules {
        nested_loop: true,
        clone_in_loop: true,
    };

    fn probe_if_not_test(file: &File, rel: &str) -> Vec<PerfHit> {
        if is_test_path(rel) {
            return Vec::new();
        }
        scan(file, rel, PROBE)
    }

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
    fn an_owned_row_clone_is_not_a_finding() {
        let file = syn::parse_file(CLONE_SRC).unwrap();
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
    fn test_paths_are_not_scanned() {
        assert!(is_test_path("tests/rows.rs"));
        assert!(is_test_path("src/rows_test.rs"));
        assert!(is_test_path("test/rows.rs"));
        assert!(is_test_path("crates/foo/tests/it.rs"));
        assert!(!is_test_path("src/lib.rs"));
        assert!(!is_test_path("src/rows.rs"));

        let file = syn::parse_file(CLONE_SRC).unwrap();
        // Probe would fire on product code…
        assert!(
            !scan(&file, "src/lib.rs", PROBE).is_empty(),
            "probe must see the clone so the skip is observable"
        );
        // …but test paths stay empty even under the probe.
        assert!(probe_if_not_test(&file, "tests/rows.rs").is_empty());
        assert!(probe_if_not_test(&file, "src/rows_test.rs").is_empty());
        assert!(perf_in_file(&file, "tests/rows.rs").is_empty());
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
        let hits = scan(&file, "src/lib.rs", PROBE);
        assert!(
            hits.iter().any(|hit| hit.symbol == "product"),
            "product clone should be visible to the probe: {hits:?}"
        );
        assert!(
            hits.iter().all(|hit| hit.symbol != "tests::helper"),
            "cfg(test) helper must be skipped: {hits:?}"
        );
        assert!(perf_in_file(&file, "src/lib.rs").is_empty());
    }
}

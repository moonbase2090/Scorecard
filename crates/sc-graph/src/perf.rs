use sc_core::Span;
use serde::{Deserialize, Serialize};
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{Expr, File, ImplItem, Item, TraitItem};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerfHit {
    pub file: String,
    pub symbol: String,
    pub rule: String,
    pub message: String,
    pub span: Span,
}

pub fn perf_in_file(file: &File, rel: &str) -> Vec<PerfHit> {
    let mut out = Vec::new();
    walk_items(&file.items, rel, "", &mut out);
    out
}

fn walk_items(items: &[Item], file: &str, prefix: &str, out: &mut Vec<PerfHit>) {
    for item in items {
        match item {
            Item::Fn(func) => {
                let symbol = qualify(prefix, &func.sig.ident.to_string());
                scan_block(file, &symbol, &func.block, out);
            }
            Item::Impl(imp) => {
                let ty = imp
                    .self_ty
                    .span()
                    .source_text()
                    .unwrap_or_else(|| "Self".into());
                let ty = ty.split('<').next().unwrap_or("Self").trim();
                let impl_prefix = qualify(prefix, ty);
                for item in &imp.items {
                    if let ImplItem::Fn(method) = item {
                        let symbol = qualify(&impl_prefix, &method.sig.ident.to_string());
                        scan_block(file, &symbol, &method.block, out);
                    }
                }
            }
            Item::Trait(trait_item) => {
                let trait_prefix = qualify(prefix, &trait_item.ident.to_string());
                for item in &trait_item.items {
                    if let TraitItem::Fn(method) = item {
                        if let Some(block) = &method.default {
                            let symbol = qualify(&trait_prefix, &method.sig.ident.to_string());
                            scan_block(file, &symbol, block, out);
                        }
                    }
                }
            }
            Item::Mod(module) => {
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

impl<'a> PerfVisitor<'a> {
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
                        "perf.nested_loop",
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
                        "perf.nested_loop",
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
                        "perf.nested_loop",
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
                    "perf.clone_in_loop",
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

    #[test]
    fn flags_nested_loop_and_clone() {
        let src = r#"
pub fn walk(items: &[String]) {
    for item in items {
        for ch in item.chars() {
            let _owned = item.clone();
            let _ = ch;
        }
    }
}
"#;
        let file = syn::parse_file(src).unwrap();
        let hits = perf_in_file(&file, "src/lib.rs");
        let rules: Vec<_> = hits.iter().map(|hit| hit.rule.as_str()).collect();
        assert!(rules.contains(&"perf.nested_loop"), "{rules:?}");
        assert!(rules.contains(&"perf.clone_in_loop"), "{rules:?}");
    }
}

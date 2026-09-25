use ca_core::Span;
use serde::{Deserialize, Serialize};
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{BinOp, Block, Expr, File, ImplItem, Item, TraitItem};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionInfo {
    pub file: String,
    pub symbol: String,
    pub span: Span,
    pub cc: u32,
}

pub fn functions_in_source(file: &File, rel_path: &str) -> Vec<FunctionInfo> {
    let mut out = Vec::new();
    collect_items(&file.items, rel_path, &module_prefix(rel_path), &mut out);
    out
}

pub fn cyclomatic_of_block(block: &Block) -> u32 {
    let mut visitor = CcVisitor { cc: 1 };
    visitor.visit_block(block);
    visitor.cc
}

fn collect_items(items: &[Item], file: &str, prefix: &str, out: &mut Vec<FunctionInfo>) {
    for item in items {
        match item {
            Item::Fn(func) => {
                if skip_attrs(&func.attrs) {
                    continue;
                }
                push_fn(
                    file,
                    prefix,
                    &func.sig.ident.to_string(),
                    func.span(),
                    &func.block,
                    out,
                );
            }
            Item::Impl(imp) => {
                if is_cfg_test(&imp.attrs) {
                    continue;
                }
                let ty = type_name(&imp.self_ty);
                let impl_prefix = qualify(prefix, &ty);
                for item in &imp.items {
                    if let ImplItem::Fn(method) = item {
                        if skip_attrs(&method.attrs) {
                            continue;
                        }
                        push_fn(
                            file,
                            &impl_prefix,
                            &method.sig.ident.to_string(),
                            method.span(),
                            &method.block,
                            out,
                        );
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
                        if skip_attrs(&method.attrs) {
                            continue;
                        }
                        if let Some(block) = &method.default {
                            push_fn(
                                file,
                                &trait_prefix,
                                &method.sig.ident.to_string(),
                                method.span(),
                                block,
                                out,
                            );
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
                    collect_items(items, file, &next, out);
                }
            }
            _ => {}
        }
    }
}

fn push_fn(
    file: &str,
    prefix: &str,
    name: &str,
    span: proc_macro2::Span,
    block: &Block,
    out: &mut Vec<FunctionInfo>,
) {
    let start = span.start();
    let end = span.end();
    out.push(FunctionInfo {
        file: file.to_string(),
        symbol: qualify(prefix, name),
        span: Span {
            start_line: start.line as u32,
            start_col: start.column as u32 + 1,
            end_line: end.line as u32,
            end_col: end.column as u32 + 1,
        },
        cc: cyclomatic_of_block(block),
    });
}

fn module_prefix(rel_path: &str) -> String {
    let rel = rel_path.trim_start_matches("./");
    let without_src = rel.strip_prefix("src/").unwrap_or(rel);
    if without_src == "lib.rs" || without_src == "main.rs" {
        return String::new();
    }
    let no_ext = without_src.trim_end_matches(".rs");
    let no_ext = no_ext.strip_suffix("/mod").unwrap_or(no_ext);
    no_ext.replace('/', "::")
}

fn qualify(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}::{name}")
    }
}

fn type_name(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
            .unwrap_or_else(|| "Self".to_string()),
        syn::Type::Reference(reference) => type_name(&reference.elem),
        syn::Type::Group(group) => type_name(&group.elem),
        syn::Type::Paren(paren) => type_name(&paren.elem),
        _ => "impl".to_string(),
    }
}

fn skip_attrs(attrs: &[syn::Attribute]) -> bool {
    is_cfg_test(attrs) || attrs.iter().any(|attr| attr.path().is_ident("test"))
}

fn is_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("cfg") {
            return false;
        }
        matches!(attr.parse_args::<syn::Ident>(), Ok(ident) if ident == "test")
    })
}

struct CcVisitor {
    cc: u32,
}

impl<'ast> Visit<'ast> for CcVisitor {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        match expr {
            Expr::If(_) | Expr::While(_) | Expr::Loop(_) | Expr::ForLoop(_) => self.cc += 1,
            Expr::Match(expr_match) => self.cc += expr_match.arms.len() as u32,
            Expr::Binary(binary) => {
                if matches!(binary.op, BinOp::And(_) | BinOp::Or(_)) {
                    self.cc += 1;
                }
            }
            Expr::Try(_) | Expr::TryBlock(_) => self.cc += 1,
            _ => {}
        }
        syn::visit::visit_expr(self, expr);
    }

    fn visit_item(&mut self, _item: &'ast Item) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLASSIFY: &str = r#"
pub fn classify(n: i32, flag: bool, mode: u8) -> &'static str {
    if n < 0 {
        return "neg";
    }
    if n == 0 {
        return "zero";
    }
    if flag && n > 100 {
        return "bigflag";
    }
    if flag || n > 50 {
        return "mid";
    }
    match mode {
        0 => "a",
        1 => "b",
        2 => "c",
        _ => "d",
    }
}
"#;

    #[test]
    fn classify_cyclomatic_complexity_is_11() {
        let file = syn::parse_file(CLASSIFY).unwrap();
        let fns = functions_in_source(&file, "src/lib.rs");
        assert_eq!(fns.len(), 1);
        assert_eq!(fns[0].symbol, "classify");
        assert_eq!(fns[0].cc, 11);
        assert_eq!(fns[0].span.start_line, 2);
        assert_eq!(fns[0].span.start_col, 1);
    }

    #[test]
    fn skips_cfg_test_modules_and_keeps_methods() {
        let src = r#"
pub fn real() -> i32 { 1 }

impl Foo {
    fn method(&self, n: i32) -> i32 {
        if n > 0 { n } else { 0 }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn t() {}
}
"#;
        let file = syn::parse_file(src).unwrap();
        let fns = functions_in_source(&file, "src/lib.rs");
        let symbols: Vec<_> = fns.iter().map(|f| f.symbol.as_str()).collect();
        assert_eq!(symbols, vec!["real", "Foo::method"]);
        assert_eq!(
            fns.iter().find(|f| f.symbol == "Foo::method").unwrap().cc,
            2
        );
    }

    #[test]
    fn file_module_prefix() {
        let src = "pub fn inner() {}";
        let file = syn::parse_file(src).unwrap();
        let fns = functions_in_source(&file, "src/foo/bar.rs");
        assert_eq!(fns[0].symbol, "foo::bar::inner");
    }
}

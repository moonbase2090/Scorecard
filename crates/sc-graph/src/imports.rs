// SPDX-License-Identifier: MPL-2.0
use serde::{Deserialize, Serialize};
use syn::{Item, UseTree};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportHit {
    pub file: String,
    pub crate_name: String,
    pub line: u32,
}

pub fn imports_in_file(file: &syn::File, rel: &str) -> Vec<ImportHit> {
    let mut out = Vec::new();
    for item in &file.items {
        match item {
            Item::Use(use_item) => {
                let mut names = Vec::new();
                crate_names(&use_item.tree, None, &mut names);
                let line = use_item.use_token.span.start().line as u32;
                for name in names {
                    if is_builtin(&name) {
                        continue;
                    }
                    out.push(ImportHit {
                        file: rel.to_string(),
                        crate_name: name,
                        line,
                    });
                }
            }
            Item::ExternCrate(extern_crate) => {
                let name = extern_crate.ident.to_string();
                if name == "self" || is_builtin(&name) {
                    continue;
                }
                out.push(ImportHit {
                    file: rel.to_string(),
                    crate_name: name,
                    line: extern_crate.extern_token.span.start().line as u32,
                });
            }
            _ => {}
        }
    }
    out.sort_by(|a, b| (&a.crate_name, a.line).cmp(&(&b.crate_name, b.line)));
    out.dedup();
    out
}

fn crate_names(tree: &UseTree, prefix: Option<&str>, out: &mut Vec<String>) {
    match tree {
        UseTree::Path(path) => {
            let ident = path.ident.to_string();
            let next = if prefix.is_none() { Some(ident) } else { None };
            let prefix = next.as_deref().or(prefix);
            crate_names(&path.tree, prefix, out);
        }
        // A bare `use Name;` or `use Name as Alias;` re-exports a name that is
        // already in scope. Only a path or `extern crate` names a crate.
        UseTree::Name(_) | UseTree::Rename(_) => {
            if let Some(prefix) = prefix {
                out.push(prefix.to_string());
            }
        }
        UseTree::Glob(_) => {
            if let Some(prefix) = prefix {
                out.push(prefix.to_string());
            }
        }
        UseTree::Group(group) => {
            for item in &group.items {
                crate_names(item, prefix, out);
            }
        }
    }
}

fn is_builtin(name: &str) -> bool {
    matches!(
        name,
        "self" | "super" | "crate" | "std" | "core" | "alloc" | "proc_macro"
    )
}

/// First segments that can never be external crates: modules declared in
/// this file (`mod name;` or `mod name { ... }`, also inside a macro call such
/// as `cfg_if! { mod name { ... } }`) and `extern crate` rename targets
/// (`extern crate foo as bar;` used as `bar::...`).
pub fn local_names_in_file(file: &syn::File) -> Vec<String> {
    let mut out = Vec::new();
    local_names(&file.items, &mut out);
    out
}

fn local_names(items: &[Item], out: &mut Vec<String>) {
    for item in items {
        match item {
            Item::Macro(item_macro) => {
                if let Ok(inner) = syn::parse2::<syn::File>(item_macro.mac.tokens.clone()) {
                    local_names(&inner.items, out);
                }
            }
            Item::Mod(item_mod) => {
                let name = item_mod.ident.to_string();
                if !out.contains(&name) {
                    out.push(name);
                }
            }
            Item::ExternCrate(extern_crate) => {
                if let Some((_, rename)) = &extern_crate.rename {
                    let name = rename.to_string();
                    if !out.contains(&name) {
                        out.push(name);
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_external_use_and_skips_crate_and_std() {
        let src = r#"
use std::fmt::Display;
use crate::local;
use missing_crate::Thing;
#[cfg(any())]
use also_missing::Other;
"#;
        let file = syn::parse_file(src).unwrap();
        let hits = imports_in_file(&file, "src/lib.rs");
        let names: Vec<_> = hits.iter().map(|hit| hit.crate_name.as_str()).collect();
        assert_eq!(names, vec!["also_missing", "missing_crate"]);
    }

    #[test]
    fn local_names_covers_mod_decls_and_extern_aliases() {
        let src = r#"
mod score;
mod inline {
    pub fn f() {}
}
extern crate serde as serde_alias;
extern crate self as this_crate;
"#;
        let file = syn::parse_file(src).unwrap();
        assert_eq!(
            local_names_in_file(&file),
            vec!["score", "inline", "serde_alias", "this_crate"]
        );
    }

    #[test]
    fn proc_macro_and_bare_reexports_are_not_crates() {
        let src = r#"
use proc_macro::TokenStream;
extern crate proc_macro;
use format::{KindFormatter, RichFormatter};
pub use KindFormatter as DefaultFormatter;
pub use RichFormatter;
use serde as serde_alias;
"#;
        let file = syn::parse_file(src).unwrap();
        let hits = imports_in_file(&file, "src/lib.rs");
        let names: Vec<_> = hits.iter().map(|hit| hit.crate_name.as_str()).collect();
        assert_eq!(names, vec!["format"]);
    }

    #[test]
    fn a_mod_declared_inside_a_macro_is_local() {
        let src = r#"
cfg_has_atomic_u64! {
    mod static_macro {
        pub struct StaticAtomicU64;
    }
}
pub(crate) use static_macro::StaticAtomicU64;
"#;
        let file = syn::parse_file(src).unwrap();
        assert_eq!(local_names_in_file(&file), vec!["static_macro"]);
    }
}

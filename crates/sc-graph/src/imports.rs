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
        UseTree::Name(name) => {
            out.push(prefix.unwrap_or(&name.ident.to_string()).to_string());
        }
        UseTree::Rename(name) => {
            out.push(prefix.unwrap_or(&name.ident.to_string()).to_string());
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
    matches!(name, "self" | "super" | "crate" | "std" | "core" | "alloc")
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
}

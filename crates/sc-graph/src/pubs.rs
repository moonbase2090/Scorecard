use serde::{Deserialize, Serialize};
use syn::{Item, Visibility};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubItem {
    pub kind: String,
    pub name: String,
    pub file: String,
}

pub fn pub_items_in_file(file: &syn::File, rel: &str) -> Vec<PubItem> {
    let mut out = Vec::new();
    collect(&file.items, rel, &mut out);
    out.sort_by(|a, b| (&a.kind, &a.name).cmp(&(&b.kind, &b.name)));
    out.dedup();
    out
}

fn collect(items: &[Item], file: &str, out: &mut Vec<PubItem>) {
    for item in items {
        match item {
            Item::Fn(func) if is_pub(&func.vis) => {
                push(out, file, "fn", &func.sig.ident.to_string())
            }
            Item::Struct(item) if is_pub(&item.vis) => {
                push(out, file, "struct", &item.ident.to_string())
            }
            Item::Enum(item) if is_pub(&item.vis) => {
                push(out, file, "enum", &item.ident.to_string())
            }
            Item::Trait(item) if is_pub(&item.vis) => {
                push(out, file, "trait", &item.ident.to_string())
            }
            Item::Type(item) if is_pub(&item.vis) => {
                push(out, file, "type", &item.ident.to_string())
            }
            Item::Const(item) if is_pub(&item.vis) => {
                push(out, file, "const", &item.ident.to_string())
            }
            Item::Mod(module) => {
                if is_pub(&module.vis) {
                    push(out, file, "mod", &module.ident.to_string());
                }
                if let Some((_, items)) = &module.content {
                    collect(items, file, out);
                }
            }
            _ => {}
        }
    }
}

fn push(out: &mut Vec<PubItem>, file: &str, kind: &str, name: &str) {
    out.push(PubItem {
        kind: kind.to_string(),
        name: name.to_string(),
        file: file.to_string(),
    });
}

fn is_pub(vis: &Visibility) -> bool {
    matches!(vis, Visibility::Public(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_public_items_only() {
        let src = r#"
pub fn visible() {}
fn hidden() {}
pub struct Visible;
mod private {
    pub fn inner() {}
}
"#;
        let file = syn::parse_file(src).unwrap();
        let items = pub_items_in_file(&file, "src/lib.rs");
        let names: Vec<_> = items.iter().map(|item| item.name.as_str()).collect();
        assert!(names.contains(&"visible"));
        assert!(names.contains(&"Visible"));
        assert!(names.contains(&"inner"));
        assert!(!names.contains(&"hidden"));
    }
}

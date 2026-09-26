use std::path::Path;

use sc_core::Finding;
use sc_graph::PubItem;
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecGap {
    pub kind: String,
    pub name: String,
    pub message: String,
}

pub fn check_spec(root: &Path, spec_text: &str, items: &[PubItem]) -> (Vec<SpecGap>, Vec<Finding>) {
    let mut gaps = Vec::new();
    let mut findings = Vec::new();
    for path in mentioned_paths(spec_text) {
        if path.starts_with("http://") || path.starts_with("https://") {
            continue;
        }
        if !root.join(&path).is_file() {
            let gap = SpecGap {
                kind: "file".into(),
                name: path.clone(),
                message: format!("spec names `{path}`, and that file is missing"),
            };
            findings.push(gap_finding(&gap));
            gaps.push(gap);
        }
    }
    for (kind, name) in mentioned_items(spec_text) {
        let found = items
            .iter()
            .any(|item| item.kind == kind && item.name == name);
        if !found {
            let label = format!("{kind} {name}");
            let gap = SpecGap {
                kind: "item".into(),
                name: label.clone(),
                message: format!("spec names public `{label}`, and it was not found"),
            };
            findings.push(gap_finding(&gap));
            gaps.push(gap);
        }
    }
    (gaps, findings)
}

pub fn gap_value(gap: &SpecGap) -> Value {
    json!({
        "kind": gap.kind,
        "name": gap.name,
        "message": gap.message,
    })
}

fn gap_finding(gap: &SpecGap) -> Finding {
    let rule = if gap.kind == "file" {
        "spec.missing_file"
    } else {
        "spec.missing_item"
    };
    Finding {
        id: format!("spec:{}:{}", gap.kind, gap.name.replace(' ', "_")),
        rule: rule.into(),
        engine: "spec".into(),
        severity: "error".into(),
        file: if gap.kind == "file" {
            gap.name.clone()
        } else {
            ".".into()
        },
        span: None,
        symbol: Some(gap.name.clone()),
        message: gap.message.clone(),
        evidence: gap_value(gap),
        suggested_action: Some(
            "Add the file or public item named by the spec, or edit the spec".into(),
        ),
        disposition: String::new(),
    }
}

fn mentioned_paths(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in tokens(text) {
        let token =
            token.trim_matches(|c: char| c == '`' || c == '"' || c == '\'' || c == '(' || c == ')');
        if token.contains("://") {
            continue;
        }
        let looks_like_path =
            token.contains('/') && token.split('/').next_back().unwrap_or("").contains('.');
        if looks_like_path {
            out.push(token.to_string());
        }
    }
    out.sort();
    out.dedup();
    out
}

fn mentioned_items(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let kinds = ["fn", "struct", "enum", "trait", "type", "const"];
    let mut index = 0;
    while index < text.len() {
        for kind in kinds {
            let marker = format!("{kind} ");
            if text[index..].starts_with(&marker) {
                let rest = &text[index + marker.len()..];
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    out.push((kind.to_string(), name));
                }
                break;
            }
        }
        index += 1;
    }
    out.sort();
    out.dedup();
    out
}

fn tokens(text: &str) -> Vec<&str> {
    text.split_whitespace().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_a_missing_file_and_item() {
        let dir = std::env::temp_dir().join(format!("sc-spec-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn kept() {}\n").unwrap();
        let items = vec![PubItem {
            kind: "fn".into(),
            name: "kept".into(),
            file: "src/lib.rs".into(),
        }];
        let spec = "See `src/missing.rs` and fn kept plus fn absent_item.\n";
        let (gaps, findings) = check_spec(&dir, spec, &items);
        assert!(gaps.iter().any(|gap| gap.name == "src/missing.rs"));
        assert!(gaps.iter().any(|gap| gap.name == "fn absent_item"));
        assert!(!gaps.iter().any(|gap| gap.name.contains("kept")));
        assert_eq!(findings.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

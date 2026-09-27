// SPDX-License-Identifier: MPL-2.0
//! Internal link check. No network requests.

use std::path::Path;

use sc_core::{Finding, Span};

use crate::html_doc::Elem;

pub fn link_findings(root: &Path, file: &str, elements: &[Elem]) -> Vec<Finding> {
    let mut findings = Vec::new();
    let html_path = root.join(file);
    for elem in elements {
        for (key, value) in &elem.attrs {
            if !matches!(key.as_str(), "href" | "src") {
                continue;
            }
            if !matches!(
                elem.name.as_str(),
                "a" | "link" | "script" | "img" | "source" | "video" | "audio" | "track"
            ) && key != "src"
                && key != "href"
            {
                continue;
            }
            let Some(rel) = local_target(value) else {
                continue;
            };
            let target = resolve(root, &html_path, &rel);
            if target.is_file() || target.is_dir() {
                continue;
            }
            findings.push(Finding {
                id: format!("links:{file}:{}:{rel}", elem.line),
                rule: "links.missing".into(),
                engine: "links".into(),
                severity: "warning".into(),
                file: file.to_string(),
                span: Some(Span {
                    start_line: elem.line.max(1),
                    start_col: elem.col.max(1),
                    end_line: elem.line.max(1),
                    end_col: elem.col.max(1).saturating_add(1),
                }),
                symbol: None,
                message: format!("missing file `{rel}`"),
                evidence: serde_json::json!({"target": rel}),
                suggested_action: Some(
                    "Point the reference at a file in the tree, or drop it.".into(),
                ),
                disposition: String::new(),
            });
        }
    }
    findings
}

fn local_target(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.starts_with('#') || value.starts_with("//") {
        return None;
    }
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("http:")
        || lower.starts_with("https:")
        || lower.starts_with("mailto:")
        || lower.starts_with("tel:")
        || lower.starts_with("data:")
        || lower.starts_with("javascript:")
        || lower.starts_with("blob:")
    {
        return None;
    }
    let without_query = value.split(['?', '#']).next().unwrap_or(value);
    if without_query.is_empty() {
        return None;
    }
    Some(without_query.to_string())
}

fn resolve(root: &Path, html_path: &Path, rel: &str) -> std::path::PathBuf {
    if rel.starts_with('/') {
        return root.join(rel.trim_start_matches('/'));
    }
    html_path.parent().unwrap_or(root).join(rel)
}

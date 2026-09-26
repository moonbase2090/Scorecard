// SPDX-License-Identifier: MPL-2.0
use std::collections::BTreeSet;

pub fn package_name(text: &str) -> Option<String> {
    let mut in_package = false;
    for line in text.lines() {
        let trimmed = strip_comment(line).trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
            continue;
        }
        if in_package {
            if let Some(name) = table_string(trimmed, "name") {
                return Some(normalize(&name));
            }
        }
    }
    None
}

pub fn dependency_names(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut in_deps = false;
    for line in text.lines() {
        let trimmed = strip_comment(line).trim();
        if trimmed.starts_with('[') {
            in_deps = is_dep_table(trimmed);
            continue;
        }
        if !in_deps || trimmed.is_empty() {
            continue;
        }
        let key = trimmed.split(['=', '.']).next().unwrap_or("").trim();
        if key.is_empty() || key.contains(' ') || key.contains('"') {
            continue;
        }
        names.insert(normalize(key));
    }
    names
}

fn is_dep_table(header: &str) -> bool {
    let header = header.trim_matches(|c| c == '[' || c == ']');
    header == "dependencies"
        || header == "dev-dependencies"
        || header == "build-dependencies"
        || header.ends_with(".dependencies")
        || header.ends_with(".dev-dependencies")
        || header.ends_with(".build-dependencies")
}

fn table_string(line: &str, key: &str) -> Option<String> {
    let (left, right) = line.split_once('=')?;
    if left.trim() != key {
        return None;
    }
    let right = right.trim().trim_matches('"').trim_matches('\'');
    if right.is_empty() {
        None
    } else {
        Some(right.to_string())
    }
}

fn strip_comment(line: &str) -> &str {
    match line.find('#') {
        Some(index) => &line[..index],
        None => line,
    }
}

pub fn normalize(name: &str) -> String {
    name.replace('-', "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_package_and_dependencies() {
        let text = r#"
[package]
name = "fake-dep"

[dependencies]
serde = "1"
other = { version = "1" }

[dev-dependencies]
temp = "0.1"
"#;
        assert_eq!(package_name(text).as_deref(), Some("fake_dep"));
        let deps = dependency_names(text);
        assert!(deps.contains("serde"));
        assert!(deps.contains("other"));
        assert!(deps.contains("temp"));
    }
}

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

/// Dependency and package names from every `Cargo.toml` under `root`.
/// Member crates and `[workspace.dependencies]` count. `target` and dot
/// directories are skipped.
pub fn manifest_crate_names(root: &std::path::Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    collect_manifest_names(root, 0, &mut names);
    names
}

fn collect_manifest_names(dir: &std::path::Path, depth: u32, names: &mut BTreeSet<String>) {
    if depth > 6 {
        return;
    }
    add_manifest_file(&dir.join("Cargo.toml"), names);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        walk_child(&entry.path(), depth, names);
    }
}

fn walk_child(path: &std::path::Path, depth: u32, names: &mut BTreeSet<String>) {
    if path.is_dir() && !skip_manifest_dir(path) {
        collect_manifest_names(path, depth + 1, names);
    }
}

fn add_manifest_file(path: &std::path::Path, names: &mut BTreeSet<String>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    names.extend(dependency_names(&text));
    if let Some(name) = package_name(&text) {
        names.insert(name);
    }
}

fn skip_manifest_dir(path: &std::path::Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return true;
    };
    name.starts_with('.') || name == "target" || name == "node_modules"
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
    fn member_manifests_count_as_declared_crates() {
        let dir = std::env::temp_dir().join(format!("sc-manifests-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("host")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers = [\"host\"]\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("host/Cargo.toml"),
            "[package]\nname = \"sc-core\"\nversion = \"0.1.0\"\n\n[dependencies]\nprismattyc-mux = { path = \"../mux\" }\nhtml5ever = \"0.1\"\n",
        )
        .unwrap();
        let names = manifest_crate_names(&dir);
        assert!(names.contains("sc_core"), "{names:?}");
        assert!(names.contains("prismattyc_mux"), "{names:?}");
        assert!(names.contains("html5ever"), "{names:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

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

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

/// Names this source file may import: its own package, that package's
/// dependencies, and `[workspace.dependencies]`. Another crate's dependencies
/// do not count.
pub fn declared_for_file(root: &std::path::Path, file_rel: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    if let Some(path) = owning_manifest(root, file_rel) {
        if let Ok(text) = std::fs::read_to_string(&path) {
            names.extend(dependency_names(&text));
            if let Some(name) = package_name(&text) {
                names.insert(name);
            }
        }
    }
    if let Ok(text) = std::fs::read_to_string(root.join("Cargo.toml")) {
        names.extend(workspace_dependency_names(&text));
    }
    names
}

fn owning_manifest(root: &std::path::Path, file_rel: &str) -> Option<std::path::PathBuf> {
    let mut dir = root.join(file_rel);
    if dir.extension().is_some() {
        dir = dir.parent()?.to_path_buf();
    }
    loop {
        let manifest = dir.join("Cargo.toml");
        if manifest.is_file() {
            if let Ok(text) = std::fs::read_to_string(&manifest) {
                if package_name(&text).is_some() {
                    return Some(manifest);
                }
            }
        }
        if dir == root {
            break;
        }
        dir = dir.parent()?.to_path_buf();
    }
    let root_manifest = root.join("Cargo.toml");
    root_manifest.is_file().then_some(root_manifest)
}

pub fn workspace_dependency_names(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut in_table = false;
    for line in text.lines() {
        let trimmed = strip_comment(line).trim();
        if trimmed.starts_with('[') {
            let header = trimmed.trim_matches(|c| c == '[' || c == ']');
            in_table = header == "workspace.dependencies";
            if header.starts_with("workspace.dependencies.") {
                if let Some(name) = dep_table_name(trimmed) {
                    names.insert(normalize(&name));
                }
            }
            continue;
        }
        if !in_table || trimmed.is_empty() {
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

pub fn dependency_names(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut in_deps = false;
    for line in text.lines() {
        let trimmed = strip_comment(line).trim();
        if trimmed.starts_with('[') {
            in_deps = is_dep_table(trimmed);
            if let Some(name) = dep_table_name(trimmed) {
                names.insert(normalize(&name));
            }
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

/// `NAME` from a header such as `[dependencies.NAME]` or
/// `[target.'cfg(windows)'.dependencies.NAME]`, which declares one crate.
fn dep_table_name(header: &str) -> Option<String> {
    let header = header.trim_matches(|c| c == '[' || c == ']');
    let parts = header_segments(header);
    let [.., table, name] = parts.as_slice() else {
        return None;
    };
    matches!(
        table.as_str(),
        "dependencies" | "dev-dependencies" | "build-dependencies"
    )
    .then(|| name.clone())
}

/// Dotted header segments, with quotes removed. A dot inside quotes does not
/// split, so `'cfg(target_os = "a.b")'` stays one segment.
fn header_segments(header: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    for c in header.chars() {
        match (quote, c) {
            (None, '\'' | '"') => quote = Some(c),
            (Some(open), _) if c == open => quote = None,
            (None, '.') => parts.push(std::mem::take(&mut current).trim().to_string()),
            _ => current.push(c),
        }
    }
    parts.push(current.trim().to_string());
    parts
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
    fn a_dependency_named_in_a_table_header_is_declared() {
        let text = r#"
[package]
name = "tokio"

[target.'cfg(windows)'.dependencies.windows-sys]
version = "0.52"
features = ["Win32_Foundation"]

[dependencies.bytes]
version = "1"

[dev-dependencies.tokio-test]
version = "0.4"

[target."cfg(target_os = \"a.b\")".build-dependencies.cc]
version = "1"
"#;
        let names = dependency_names(text);
        for name in ["windows_sys", "bytes", "tokio_test", "cc"] {
            assert!(names.contains(name), "{name} missing from {names:?}");
        }
        assert!(!names.contains("version"), "{names:?}");
        assert!(!names.contains("features"), "{names:?}");
    }

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
        let names = declared_for_file(&dir, "host/src/lib.rs");
        assert!(names.contains("sc_core"), "{names:?}");
        assert!(names.contains("prismattyc_mux"), "{names:?}");
        assert!(names.contains("html5ever"), "{names:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sibling_crate_dependency_is_not_declared_for_this_file() {
        let dir = std::env::temp_dir().join(format!("sc-per-crate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a/src")).unwrap();
        std::fs::create_dir_all(dir.join("b/src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers = [\"a\", \"b\"]\n\n[workspace.dependencies]\nserde = \"1\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("a/Cargo.toml"),
            "[package]\nname = \"crate-a\"\nversion = \"0.1.0\"\n\n[dependencies]\ntokio = \"1\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("b/Cargo.toml"),
            "[package]\nname = \"crate-b\"\nversion = \"0.1.0\"\n\n[dependencies]\nbytes = \"1\"\n",
        )
        .unwrap();
        let a = declared_for_file(&dir, "a/src/lib.rs");
        assert!(a.contains("tokio"), "{a:?}");
        assert!(a.contains("crate_a"), "{a:?}");
        assert!(a.contains("serde"), "{a:?}");
        assert!(!a.contains("bytes"), "{a:?}");
        let b = declared_for_file(&dir, "b/src/lib.rs");
        assert!(b.contains("bytes"), "{b:?}");
        assert!(!b.contains("tokio"), "{b:?}");
        assert!(b.contains("serde"), "{b:?}");
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

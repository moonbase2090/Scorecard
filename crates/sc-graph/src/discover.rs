// SPDX-License-Identifier: MPL-2.0
use std::fs;
use std::path::Path;

use crate::complexity::functions_in_source;
use crate::FunctionInfo;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scan {
    pub functions: Vec<FunctionInfo>,
    pub paths: Vec<String>,
    pub files: u64,
    pub loc: u64,
}

pub fn source_files(root: &Path, exclude: &[String]) -> Vec<std::path::PathBuf> {
    let dirs = [root.join("src")];
    source_files_under(root, &dirs, exclude)
}

/// Rust files under each directory, relative to `root` after the walk.
///
/// A single crate passes `root/src`. A Cargo workspace also passes each
/// member's `src`. Overlapping directories are scanned once.
pub fn source_files_under(
    root: &Path,
    src_dirs: &[std::path::PathBuf],
    exclude: &[String],
) -> Vec<std::path::PathBuf> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut files = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for src in src_dirs {
        let src = src.canonicalize().unwrap_or_else(|_| src.clone());
        if !src.is_dir() || !seen.insert(src.clone()) {
            continue;
        }
        walk(&src, &root, exclude, &mut files);
    }
    files.sort();
    files.dedup();
    files
}

pub fn scan(root: &Path, exclude: &[String]) -> Scan {
    let files = source_files(root, exclude);

    let mut functions = Vec::new();
    let mut loc = 0u64;
    let mut paths = Vec::new();
    for path in &files {
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        loc += text.lines().count() as u64;
        paths.push(rel.clone());
        if let Ok(file) = syn::parse_file(&text) {
            functions.extend(functions_in_source(&file, &rel));
        }
    }
    paths.sort();
    functions.sort_by(|a, b| (&a.file, &a.symbol).cmp(&(&b.file, &b.symbol)));

    Scan {
        functions,
        files: paths.len() as u64,
        loc,
        paths,
    }
}

fn walk(dir: &Path, root: &Path, exclude: &[String], out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if is_excluded(&rel, exclude) {
            continue;
        }
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if path.is_dir() {
            if name == ".git" {
                continue;
            }
            walk(&path, root, exclude, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

pub fn is_excluded(rel: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pattern| glob_hit(pattern, rel))
}

fn glob_hit(pattern: &str, path: &str) -> bool {
    let pattern = pattern.trim().trim_start_matches("./");
    if pattern.is_empty() {
        return false;
    }
    if let Some(body) = pattern.strip_suffix("/**") {
        let body = body.trim_start_matches("**/").trim_matches('/');
        if body.is_empty() {
            return true;
        }
        let parts: Vec<&str> = body.split('/').filter(|s| !s.is_empty()).collect();
        let comps: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        return comps
            .windows(parts.len())
            .any(|window| window == parts.as_slice());
    }
    path == pattern || path.starts_with(&format!("{pattern}/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_and_generated_are_excluded() {
        let patterns = vec!["target/**".into(), "generated/**".into()];
        assert!(is_excluded("target/debug/lib.rs", &patterns));
        assert!(is_excluded("generated/foo.rs", &patterns));
        assert!(!is_excluded("src/lib.rs", &patterns));
    }
}

// SPDX-License-Identifier: MPL-2.0
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use sc_core::Finding;
use sc_graph::{source_files_under, FunctionInfo, ImportHit, PerfHit, PubItem};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::secrets::secrets_in_text;

const CACHE_VERSION: u32 = 2;

#[derive(Debug, Clone)]
pub struct AnalyzedFile {
    pub rel: String,
    pub loc: u64,
    pub functions: Vec<FunctionInfo>,
    pub imports: Vec<ImportHit>,
    pub perf: Vec<PerfHit>,
    pub items: Vec<PubItem>,
    pub secrets: Vec<Finding>,
    /// `mod` declarations and `extern crate` renames in this file.
    pub local_names: Vec<String>,
}

/// Analyze project sources and return whether `root` is the Cargo workspace root.
pub(crate) fn analyze_tree_with_workspace(
    root: &Path,
    exclude: &[String],
) -> (Vec<AnalyzedFile>, bool) {
    let metadata = cargo_metadata(root);
    let workspace_root = metadata
        .as_ref()
        .is_some_and(|meta| metadata_is_workspace_root(root, meta));
    let mut dirs = vec![root.join("src")];
    if workspace_root {
        dirs.extend(member_src_dirs(metadata.as_ref()));
    }
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let paths = source_files_under(&root, &dirs, exclude);
    let rels: Vec<String> = paths
        .iter()
        .map(|path| {
            path.strip_prefix(&root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    (analyze_rels(&root, &rels), workspace_root)
}

pub fn analyze_rels(root: &Path, rels: &[String]) -> Vec<AnalyzedFile> {
    let cache_path = cache_path(root);
    let mut cache = read_cache(&cache_path);
    let mut out = Vec::new();
    for rel in rels {
        let path = root.join(rel);
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let hash = hash_text(&text);
        if let Some(hit) = cache.files.get(rel) {
            if hit.hash == hash {
                out.push(hit.to_analyzed(rel));
                continue;
            }
        }
        let analyzed = parse_file(rel, &text);
        cache
            .files
            .insert(rel.clone(), CachedFile::from_analyzed(&hash, &analyzed));
        out.push(analyzed);
    }
    let _ = write_cache(&cache_path, &cache);
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

fn member_src_dirs(meta: Option<&CargoMetadata>) -> Vec<PathBuf> {
    let Some(meta) = meta else {
        return Vec::new();
    };
    let members: std::collections::BTreeSet<&str> =
        meta.workspace_members.iter().map(String::as_str).collect();
    let mut dirs = Vec::new();
    for package in &meta.packages {
        if !members.contains(package.id.as_str()) {
            continue;
        }
        let Some(dir) = Path::new(&package.manifest_path).parent() else {
            continue;
        };
        let src = dir.join("src");
        if src.is_dir() && !dirs.iter().any(|have: &PathBuf| have == &src) {
            dirs.push(src);
        }
    }
    dirs
}

pub(crate) fn is_workspace_root(root: &Path) -> bool {
    let Some(meta) = cargo_metadata(root) else {
        return false;
    };
    metadata_is_workspace_root(root, &meta)
}

fn metadata_is_workspace_root(root: &Path, meta: &CargoMetadata) -> bool {
    let workspace_root = PathBuf::from(&meta.workspace_root);
    match (root.canonicalize(), workspace_root.canonicalize()) {
        (Ok(root), Ok(workspace_root)) => root == workspace_root,
        _ => false,
    }
}

fn cargo_metadata(root: &Path) -> Option<CargoMetadata> {
    if !root.join("Cargo.toml").is_file() {
        return None;
    }
    let mut cmd = crate::command::cargo_command(root);
    cmd.args([
        "metadata",
        "--no-deps",
        "--offline",
        "--format-version",
        "1",
    ]);
    let captured = crate::command::run_cmd(&mut cmd, std::time::Duration::from_secs(60)).ok()?;
    if !captured.status.success() {
        return None;
    }
    serde_json::from_str(&captured.stdout).ok()
}

#[derive(Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
    workspace_members: Vec<String>,
    workspace_root: String,
}

#[derive(Deserialize)]
struct CargoPackage {
    id: String,
    manifest_path: String,
}

fn parse_file(rel: &str, text: &str) -> AnalyzedFile {
    let loc = text.lines().count() as u64;
    let facts = sc_graph::inspect_source(text, rel);
    let (functions, imports, perf, items, local_names) = match facts {
        Some(facts) => (
            facts.functions,
            facts.imports,
            facts.perf,
            facts.items,
            facts.local_names,
        ),
        None => (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()),
    };
    AnalyzedFile {
        rel: rel.to_string(),
        loc,
        functions,
        imports,
        perf,
        items,
        secrets: secrets_in_text(text, rel),
        local_names,
    }
}

fn hash_text(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn cache_path(root: &Path) -> PathBuf {
    root.join(".sc").join("cache").join("parse-v2.json")
}

fn read_cache(path: &Path) -> CacheDoc {
    let Ok(text) = fs::read_to_string(path) else {
        return CacheDoc::default();
    };
    let doc: CacheDoc = serde_json::from_str(&text).unwrap_or_default();
    if doc.version != CACHE_VERSION {
        return CacheDoc::default();
    }
    doc
}

fn write_cache(path: &Path, doc: &CacheDoc) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        serde_json::to_string(doc).unwrap_or_else(|_| "{}".into()),
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheDoc {
    version: u32,
    files: BTreeMap<String, CachedFile>,
}

impl Default for CacheDoc {
    fn default() -> Self {
        Self {
            version: CACHE_VERSION,
            files: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedFile {
    hash: String,
    loc: u64,
    functions: Vec<FunctionInfo>,
    imports: Vec<ImportHit>,
    perf: Vec<PerfHit>,
    items: Vec<PubItem>,
    secrets: Vec<Finding>,
    local_names: Vec<String>,
}

impl CachedFile {
    fn from_analyzed(hash: &str, file: &AnalyzedFile) -> Self {
        Self {
            hash: hash.to_string(),
            loc: file.loc,
            functions: file.functions.clone(),
            imports: file.imports.clone(),
            perf: file.perf.clone(),
            items: file.items.clone(),
            secrets: file.secrets.clone(),
            local_names: file.local_names.clone(),
        }
    }

    fn to_analyzed(&self, rel: &str) -> AnalyzedFile {
        AnalyzedFile {
            rel: rel.to_string(),
            loc: self.loc,
            functions: self.functions.clone(),
            imports: self.imports.clone(),
            perf: self.perf.clone(),
            items: self.items.clone(),
            secrets: self.secrets.clone(),
            local_names: self.local_names.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_read_reuses_the_cache() {
        let dir = std::env::temp_dir().join(format!("sc-cache-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/lib.rs"), "pub fn cached() -> i32 { 1 }\n").unwrap();
        let first = analyze_tree_with_workspace(&dir, &[]).0;
        assert_eq!(first[0].functions[0].symbol, "cached");
        fs::write(
            dir.join(".sc/cache/parse-v2.json"),
            fs::read_to_string(dir.join(".sc/cache/parse-v2.json")).unwrap(),
        )
        .unwrap();
        let second = analyze_tree_with_workspace(&dir, &[]).0;
        assert_eq!(second[0].functions[0].symbol, "cached");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn workspace_members_are_read_from_their_src() {
        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/workspace_src");
        let dir = std::env::temp_dir().join(format!("sc-ws-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        copy_fixture(&src, &dir);
        let files = analyze_tree_with_workspace(&dir, &[]).0;
        let rels: Vec<&str> = files.iter().map(|file| file.rel.as_str()).collect();
        assert!(
            rels.iter()
                .any(|rel| rel.ends_with("crates/left/src/lib.rs")),
            "{rels:?}"
        );
        assert!(
            rels.iter()
                .any(|rel| rel.ends_with("crates/right/src/lib.rs")),
            "{rels:?}"
        );
        let symbols: Vec<&str> = files
            .iter()
            .flat_map(|file| {
                file.functions
                    .iter()
                    .map(|function| function.symbol.as_str())
            })
            .collect();
        assert!(symbols.contains(&"left_one"), "{symbols:?}");
        assert!(symbols.contains(&"right_one"), "{symbols:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    fn copy_fixture(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap().flatten() {
            let name = entry.file_name();
            if name == ".sc" || name == "target" || name == ".git" {
                continue;
            }
            let dest = to.join(&name);
            if entry.path().is_dir() {
                copy_fixture(&entry.path(), &dest);
            } else {
                fs::copy(entry.path(), dest).unwrap();
            }
        }
    }
}

// SPDX-License-Identifier: MPL-2.0
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use sc_core::Finding;
use sc_graph::{source_files_under, FunctionInfo, ImportHit, PerfHit, PubItem};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::secrets::secrets_in_text;

/// Bump when what a parse records changes, so an upgrade does not reuse old
/// facts for unchanged files.
const CACHE_VERSION: u32 = 6;

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
    toolchain_pin: &str,
    perf_enabled: bool,
) -> (Vec<AnalyzedFile>, bool) {
    let (rels, workspace_root) = tree_source_rels(root, exclude, toolchain_pin);
    (
        analyze_rels(root, &rels, perf_enabled, exclude, toolchain_pin),
        workspace_root,
    )
}

/// Count (and list) the same source paths a tree-scope run would analyze,
/// without parsing them. Diff reports use the count for the rest-of-tree line.
pub(crate) fn tree_source_rels(
    root: &Path,
    exclude: &[String],
    toolchain_pin: &str,
) -> (Vec<String>, bool) {
    let metadata = cargo_metadata(root, toolchain_pin);
    let workspace_root = metadata
        .as_ref()
        .is_some_and(|meta| metadata_is_workspace_root(root, meta));
    let mut dirs = vec![root.join("src")];
    if workspace_root {
        dirs.extend(member_src_dirs(metadata.as_ref()));
    }
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let paths = source_files_under(&root, &dirs, exclude);
    let mut rels: Vec<String> = paths
        .iter()
        .map(|path| {
            path.strip_prefix(&root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    rels.sort();
    (rels, workspace_root)
}

pub fn analyze_rels(
    root: &Path,
    rels: &[String],
    perf_enabled: bool,
    exclude: &[String],
    toolchain_pin: &str,
) -> Vec<AnalyzedFile> {
    let cache_path = cache_path(root);
    let mut cache = read_cache(&cache_path);
    if cache.perf_enabled != perf_enabled {
        cache = CacheDoc {
            version: CACHE_VERSION,
            perf_enabled,
            files: BTreeMap::new(),
        };
    }
    let mut texts: BTreeMap<String, String> = BTreeMap::new();
    for rel in rels {
        let path = root.join(rel);
        if let Ok(text) = fs::read_to_string(&path) {
            texts.insert(rel.clone(), text);
        }
    }
    let (cfg_test_files, cfg_test_prefixes) = if perf_enabled {
        let (tree_rels, _) = tree_source_rels(root, exclude, toolchain_pin);
        cfg_test_coverage_from_tree(root, &tree_rels)
    } else {
        (
            std::collections::BTreeSet::new(),
            std::collections::BTreeSet::new(),
        )
    };
    let mut out = Vec::new();
    for (rel, text) in &texts {
        let hash = hash_text(text);
        // Always record unfiltered perf when enabled so a later cfg(test)→product
        // declaration change does not leave a cleared cache entry.
        let analyzed = if let Some(hit) = cache.files.get(rel) {
            if hit.hash == hash {
                hit.to_analyzed(rel)
            } else {
                parse_file(rel, text, perf_enabled)
            }
        } else {
            parse_file(rel, text, perf_enabled)
        };
        cache
            .files
            .insert(rel.clone(), CachedFile::from_analyzed(&hash, &analyzed));
        let mut returned = analyzed;
        if sc_graph::is_cfg_test_only(rel, &cfg_test_files, &cfg_test_prefixes) {
            returned.perf.clear();
        }
        out.push(returned);
    }
    cache.perf_enabled = perf_enabled;
    let _ = write_cache(&cache_path, &cache);
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

/// Declared out-of-line `#[cfg(test)]` module files and their child-directory
/// prefixes, discovered from the whole tree (not only the analyzed subset).
fn cfg_test_coverage_from_tree(
    root: &Path,
    tree_rels: &[String],
) -> (
    std::collections::BTreeSet<String>,
    std::collections::BTreeSet<String>,
) {
    let mut declared = Vec::new();
    for rel in tree_rels {
        let Ok(text) = fs::read_to_string(root.join(rel)) else {
            continue;
        };
        declared.extend(sc_graph::out_of_line_cfg_test_paths_from_source(&text, rel));
    }
    sc_graph::cfg_test_coverage(declared)
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

pub(crate) fn is_workspace_root(root: &Path, toolchain_pin: &str) -> bool {
    let Some(meta) = cargo_metadata(root, toolchain_pin) else {
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

fn cargo_metadata(root: &Path, toolchain_pin: &str) -> Option<CargoMetadata> {
    if !root.join("Cargo.toml").is_file() {
        return None;
    }
    let mut cmd = crate::command::cargo_command(root, toolchain_pin);
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

fn parse_file(rel: &str, text: &str, perf_enabled: bool) -> AnalyzedFile {
    let loc = text.lines().count() as u64;
    let facts = sc_graph::inspect_source(text, rel, perf_enabled);
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
    /// Matches `engines.perf` for the run that wrote this cache. A mismatch
    /// drops the file entries so enabling perf re-scans.
    #[serde(default)]
    perf_enabled: bool,
    files: BTreeMap<String, CachedFile>,
}

impl Default for CacheDoc {
    fn default() -> Self {
        Self {
            version: CACHE_VERSION,
            perf_enabled: false,
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
    fn out_of_line_cfg_test_file_skips_perf() {
        let dir = std::env::temp_dir().join(format!("sc-cfg-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(
            dir.join("src/lib.rs"),
            "pub fn product(rows: &[String]) -> Vec<String> {\n    let mut out = Vec::new();\n    for row in rows { out.push(row.clone()); }\n    out\n}\n\n#[cfg(test)]\nmod helpers;\n",
        )
        .unwrap();
        fs::write(
            dir.join("src/helpers.rs"),
            "pub fn helper(rows: &[String]) -> Vec<String> {\n    let mut out = Vec::new();\n    for row in rows {\n        for _ in 0..1 { out.push(row.clone()); }\n    }\n    out\n}\n",
        )
        .unwrap();
        let files = analyze_tree_with_workspace(&dir, &[], "", true).0;
        let helpers = files
            .iter()
            .find(|file| file.rel == "src/helpers.rs")
            .expect("helpers.rs analyzed");
        assert!(
            helpers.perf.is_empty(),
            "out-of-line cfg(test) file must not get perf hits: {:?}",
            helpers.perf
        );
        let product = files
            .iter()
            .find(|file| file.rel == "src/lib.rs")
            .expect("lib.rs analyzed");
        assert!(
            product
                .perf
                .iter()
                .any(|hit| hit.symbol == "product" && hit.rule == "perf.clone_in_loop"),
            "product still scanned: {:?}",
            product.perf
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cfg_test_module_child_files_are_skipped() {
        let dir = std::env::temp_dir().join(format!("sc-cfg-child-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src/helpers")).unwrap();
        fs::write(
            dir.join("src/lib.rs"),
            "pub fn product(rows: &[String]) -> Vec<String> {\n    let mut out = Vec::new();\n    for row in rows { out.push(row.clone()); }\n    out\n}\n\n#[cfg(test)]\nmod helpers;\n",
        )
        .unwrap();
        fs::write(
            dir.join("src/helpers.rs"),
            "pub mod fixtures;\npub fn helper(rows: &[String]) -> Vec<String> {\n    let mut out = Vec::new();\n    for row in rows { out.push(row.clone()); }\n    out\n}\n",
        )
        .unwrap();
        fs::write(
            dir.join("src/helpers/fixtures.rs"),
            "pub fn fixture(rows: &[String]) -> Vec<String> {\n    let mut out = Vec::new();\n    for row in rows {\n        for _ in 0..1 { out.push(row.clone()); }\n    }\n    out\n}\n",
        )
        .unwrap();
        let files = analyze_tree_with_workspace(&dir, &[], "", true).0;
        let fixtures = files
            .iter()
            .find(|file| file.rel == "src/helpers/fixtures.rs")
            .expect("fixtures.rs analyzed");
        assert!(
            fixtures.perf.is_empty(),
            "child of cfg(test) module must not get perf hits: {:?}",
            fixtures.perf
        );
        let helpers = files
            .iter()
            .find(|file| file.rel == "src/helpers.rs")
            .expect("helpers.rs");
        assert!(helpers.perf.is_empty(), "{:?}", helpers.perf);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn diff_only_test_module_file_stays_clean() {
        let dir = std::env::temp_dir().join(format!("sc-cfg-diff-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"cfg_diff\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(
            dir.join("src/lib.rs"),
            "pub fn product(rows: &[String]) -> Vec<String> {\n    let mut out = Vec::new();\n    for row in rows { out.push(row.clone()); }\n    out\n}\n\n#[cfg(test)]\nmod helpers;\n",
        )
        .unwrap();
        fs::write(
            dir.join("src/helpers.rs"),
            "pub fn helper(rows: &[String]) -> Vec<String> {\n    let mut out = Vec::new();\n    for row in rows { out.push(row.clone()); }\n    out\n}\n",
        )
        .unwrap();
        // Analyze only helpers.rs (as --diff would when lib.rs is unchanged).
        let files = analyze_rels(&dir, &["src/helpers.rs".into()], true, &[], "");
        assert_eq!(files.len(), 1);
        assert!(
            files[0].perf.is_empty(),
            "diff of only the test-only file must stay clean: {:?}",
            files[0].perf
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_keeps_hits_when_cfg_test_becomes_product() {
        let dir = std::env::temp_dir().join(format!("sc-cfg-cache-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(
            dir.join("src/lib.rs"),
            "pub fn product() {}\n\n#[cfg(test)]\nmod helpers;\n",
        )
        .unwrap();
        fs::write(
            dir.join("src/helpers.rs"),
            "pub fn helper(rows: &[String]) -> Vec<String> {\n    let mut out = Vec::new();\n    for row in rows { out.push(row.clone()); }\n    out\n}\n",
        )
        .unwrap();
        let first = analyze_tree_with_workspace(&dir, &[], "", true).0;
        let helpers = first
            .iter()
            .find(|file| file.rel == "src/helpers.rs")
            .unwrap();
        assert!(
            helpers.perf.is_empty(),
            "cfg(test) first pass: {:?}",
            helpers.perf
        );
        // Warm cache stored unfiltered hits; flip declaration to product.
        fs::write(
            dir.join("src/lib.rs"),
            "pub fn product() {}\n\npub mod helpers;\n",
        )
        .unwrap();
        let second = analyze_tree_with_workspace(&dir, &[], "", true).0;
        let helpers = second
            .iter()
            .find(|file| file.rel == "src/helpers.rs")
            .unwrap();
        assert!(
            helpers
                .perf
                .iter()
                .any(|hit| hit.symbol == "helper" && hit.rule == "perf.clone_in_loop"),
            "after cfg(test)→product, warm cache must still yield hits: {:?}",
            helpers.perf
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn second_read_reuses_the_cache() {
        let dir = std::env::temp_dir().join(format!("sc-cache-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/lib.rs"), "pub fn cached() -> i32 { 1 }\n").unwrap();
        let first = analyze_tree_with_workspace(&dir, &[], "", false).0;
        assert_eq!(first[0].functions[0].symbol, "cached");
        fs::write(
            dir.join(".sc/cache/parse-v2.json"),
            fs::read_to_string(dir.join(".sc/cache/parse-v2.json")).unwrap(),
        )
        .unwrap();
        let second = analyze_tree_with_workspace(&dir, &[], "", false).0;
        assert_eq!(second[0].functions[0].symbol, "cached");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn workspace_members_are_read_from_their_src() {
        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/workspace_src");
        let dir = std::env::temp_dir().join(format!("sc-ws-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        copy_fixture(&src, &dir);
        let files = analyze_tree_with_workspace(&dir, &[], "", false).0;
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

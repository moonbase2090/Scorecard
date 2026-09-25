use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use ca_core::Finding;
use ca_graph::{source_files, FunctionInfo, ImportHit, PerfHit, PubItem};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::secrets::secrets_in_text;

const CACHE_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct AnalyzedFile {
    pub rel: String,
    pub loc: u64,
    pub functions: Vec<FunctionInfo>,
    pub imports: Vec<ImportHit>,
    pub perf: Vec<PerfHit>,
    pub items: Vec<PubItem>,
    pub secrets: Vec<Finding>,
}

pub fn analyze_tree(root: &Path, exclude: &[String]) -> Vec<AnalyzedFile> {
    let paths = source_files(root, exclude);
    let rels: Vec<String> = paths
        .iter()
        .map(|path| {
            path.strip_prefix(root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    analyze_rels(root, &rels)
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

fn parse_file(rel: &str, text: &str) -> AnalyzedFile {
    let loc = text.lines().count() as u64;
    let facts = ca_graph::inspect_source(text, rel);
    let (functions, imports, perf, items) = match facts {
        Some(facts) => (facts.functions, facts.imports, facts.perf, facts.items),
        None => (Vec::new(), Vec::new(), Vec::new(), Vec::new()),
    };
    AnalyzedFile {
        rel: rel.to_string(),
        loc,
        functions,
        imports,
        perf,
        items,
        secrets: secrets_in_text(text, rel),
    }
}

fn hash_text(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn cache_path(root: &Path) -> PathBuf {
    root.join(".ca").join("cache").join("parse-v1.json")
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_read_reuses_the_cache() {
        let dir = std::env::temp_dir().join(format!("ca-cache-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/lib.rs"), "pub fn cached() -> i32 { 1 }\n").unwrap();
        let first = analyze_tree(&dir, &[]);
        assert_eq!(first[0].functions[0].symbol, "cached");
        fs::write(
            dir.join(".ca/cache/parse-v1.json"),
            fs::read_to_string(dir.join(".ca/cache/parse-v1.json")).unwrap(),
        )
        .unwrap();
        let second = analyze_tree(&dir, &[]);
        assert_eq!(second[0].functions[0].symbol, "cached");
        let _ = fs::remove_dir_all(&dir);
    }
}

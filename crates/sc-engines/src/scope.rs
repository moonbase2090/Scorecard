// SPDX-License-Identifier: MPL-2.0
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use sc_graph::FunctionInfo;

use crate::command::{run_cmd, CommandError};
use crate::facts::{analyze_rels, AnalyzedFile};

#[derive(Debug, Clone)]
pub struct Selection {
    pub mode: String,
    pub files: Vec<AnalyzedFile>,
    pub crap_functions: Vec<FunctionInfo>,
    pub new_symbols: BTreeSet<(String, String)>,
    pub narrow_untested: bool,
    pub paths: Vec<String>,
    pub loc_changed: u64,
    pub files_changed: u64,
    /// Resolved diff base, only in diff mode.
    pub base: Option<String>,
    pub workspace_root: bool,
}

pub fn empty_selection() -> Selection {
    tree_like("tree", Vec::new(), false)
}

pub fn select(
    root: &Path,
    exclude: &[String],
    diff_base: Option<&str>,
    diff_head: Option<&str>,
    path_list: &[String],
) -> Result<Selection, String> {
    if diff_base.is_some() && !path_list.is_empty() {
        return Err("pass either --diff or --paths, not both".into());
    }
    if let Some(base) = diff_base {
        return select_diff(
            root,
            exclude,
            base,
            diff_head,
            crate::facts::is_workspace_root(root),
        );
    }
    if !path_list.is_empty() {
        return Ok(select_paths(
            root,
            path_list,
            crate::facts::is_workspace_root(root),
        ));
    }
    let (files, workspace_root) = crate::facts::analyze_tree_with_workspace(root, exclude);
    Ok(tree_like("tree", files, workspace_root))
}

fn tree_like(mode: &str, files: Vec<AnalyzedFile>, workspace_root: bool) -> Selection {
    let crap_functions = files
        .iter()
        .flat_map(|file| file.functions.clone())
        .collect();
    let paths = files.iter().map(|file| file.rel.clone()).collect();
    let loc_changed = files.iter().map(|file| file.loc).sum();
    let files_changed = files.len() as u64;
    Selection {
        mode: mode.to_string(),
        files,
        crap_functions,
        new_symbols: BTreeSet::new(),
        narrow_untested: false,
        paths,
        loc_changed,
        files_changed,
        base: None,
        workspace_root,
    }
}

fn select_paths(root: &Path, path_list: &[String], workspace_root: bool) -> Selection {
    let rels: Vec<String> = path_list
        .iter()
        .map(|path| normalize_rel(root, path))
        .filter(|rel| rel.ends_with(".rs"))
        .collect();
    tree_like("paths", analyze_rels(root, &rels), workspace_root)
}

fn select_diff(
    root: &Path,
    exclude: &[String],
    base: &str,
    head: Option<&str>,
    workspace_root: bool,
) -> Result<Selection, String> {
    let base = resolve_base(root, base)?;
    let deltas = diff_files(root, &base, head)?;
    let exclude_hit = |rel: &str| sc_graph::is_excluded(rel, exclude);
    let deltas: Vec<_> = deltas
        .into_iter()
        .filter(|delta| !exclude_hit(&delta.rel) && delta.rel.contains("src/"))
        .collect();
    let rels: Vec<String> = deltas.iter().map(|delta| delta.rel.clone()).collect();
    let files = analyze_rels(root, &rels);
    let mut crap_functions = Vec::new();
    let mut new_symbols = BTreeSet::new();
    let mut loc_changed = 0u64;
    for file in &files {
        let Some(delta) = deltas.iter().find(|delta| delta.rel == file.rel) else {
            continue;
        };
        let base_syms = if delta.is_new_file {
            BTreeSet::new()
        } else {
            base_symbols(root, &base, &file.rel)
        };
        for function in &file.functions {
            let is_new = delta.is_new_file || !base_syms.contains(&function.symbol);
            let changed = delta.is_new_file || overlaps(function, &delta.changed_lines);
            if is_new {
                new_symbols.insert((function.file.clone(), function.symbol.clone()));
            }
            if changed {
                crap_functions.push(function.clone());
            }
        }
        loc_changed += if delta.is_new_file {
            file.loc
        } else {
            delta.changed_lines.len() as u64
        };
    }
    let files_changed = files.len() as u64;
    let paths = files.iter().map(|file| file.rel.clone()).collect();
    Ok(Selection {
        mode: "diff".into(),
        files,
        crap_functions,
        new_symbols,
        narrow_untested: true,
        paths,
        loc_changed,
        files_changed,
        base: Some(base),
        workspace_root,
    })
}

fn normalize_rel(root: &Path, path: &str) -> String {
    let path = path.trim().replace('\\', "/");
    let as_path = Path::new(&path);
    if as_path.is_absolute() {
        return as_path
            .strip_prefix(root)
            .map(|rel| rel.to_string_lossy().replace('\\', "/"))
            .unwrap_or(path);
    }
    path.trim_start_matches("./").to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDelta {
    pub rel: String,
    pub changed_lines: Vec<u32>,
    pub is_new_file: bool,
}

pub fn resolve_base(root: &Path, requested: &str) -> Result<String, String> {
    if requested != "AUTO" {
        return Ok(requested.to_string());
    }
    for candidate in ["HEAD~1", "main", "master", "HEAD"] {
        if git_ok(root, &["rev-parse", "--verify", "--quiet", candidate]) {
            return Ok(candidate.to_string());
        }
    }
    Err("cannot resolve a diff base; pass --diff BASE".into())
}

pub fn diff_files(root: &Path, base: &str, head: Option<&str>) -> Result<Vec<FileDelta>, String> {
    let names = git_diff_names(root, base, head)?;
    let untracked = if head.is_none() {
        git(root, &["ls-files", "--others", "--exclude-standard"]).unwrap_or_default()
    } else {
        String::new()
    };
    let mut rels = BTreeSet::new();
    for line in names.lines().chain(untracked.lines()) {
        let rel = line.trim().replace('\\', "/");
        if rel.ends_with(".rs") {
            rels.insert(rel);
        }
    }
    let mut out = Vec::new();
    for rel in rels {
        let patch = git_diff_patch(root, base, head, &rel).unwrap_or_default();
        let tracked = git_ok(root, &["cat-file", "-e", &format!("{base}:{rel}")]);
        let changed_lines = if tracked {
            parse_new_lines(&patch)
        } else {
            vec![]
        };
        out.push(FileDelta {
            rel,
            changed_lines,
            is_new_file: !tracked,
        });
    }
    Ok(out)
}

pub fn parse_new_lines(patch: &str) -> Vec<u32> {
    let mut lines = Vec::new();
    for line in patch.lines() {
        let Some(rest) = line.strip_prefix("@@") else {
            continue;
        };
        let Some(plus) = rest.split_whitespace().find(|part| part.starts_with('+')) else {
            continue;
        };
        let spec = plus.trim_start_matches('+');
        let (start, count) = match spec.split_once(',') {
            Some((start, count)) => (start, count),
            None => (spec, "1"),
        };
        let Ok(start) = start.parse::<u32>() else {
            continue;
        };
        let Ok(count) = count.parse::<u32>() else {
            continue;
        };
        for line_no in start..start.saturating_add(count) {
            lines.push(line_no);
        }
    }
    lines.sort_unstable();
    lines.dedup();
    lines
}

pub fn overlaps(function: &FunctionInfo, changed: &[u32]) -> bool {
    if changed.is_empty() {
        return false;
    }
    let start = function.span.start_line;
    let end = function.span.end_line.max(start);
    changed.iter().any(|line| *line >= start && *line <= end)
}

pub fn base_symbols(root: &Path, base: &str, rel: &str) -> BTreeSet<String> {
    let text = match git(root, &["show", &format!("{base}:{rel}")]) {
        Ok(text) => text,
        Err(_) => return BTreeSet::new(),
    };
    sc_graph::function_symbols(&text, rel).into_iter().collect()
}

fn git_diff_names(root: &Path, base: &str, head: Option<&str>) -> Result<String, String> {
    let mut args = vec!["diff", "--name-only", "--diff-filter=ACMR", base];
    if let Some(head) = head {
        args.push(head);
    }
    git(root, &args)
}

fn git_diff_patch(
    root: &Path,
    base: &str,
    head: Option<&str>,
    rel: &str,
) -> Result<String, String> {
    let mut args = vec!["diff", "--unified=0", base];
    if let Some(head) = head {
        args.push(head);
    }
    args.push("--");
    args.push(rel);
    git(root, &args)
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(root)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    match run_cmd(&mut cmd, Duration::from_secs(10)) {
        Ok(captured) if captured.status.success() => Ok(captured.stdout),
        Ok(captured) => Err(captured.stderr.trim().to_string()),
        Err(CommandError::NotFound) => Err("git is not installed".into()),
        Err(CommandError::Timeout) => Err("git timed out".into()),
        Err(CommandError::Spawn(err)) => Err(err),
    }
}

fn git_ok(root: &Path, args: &[&str]) -> bool {
    git(root, args).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::Span;

    #[test]
    fn parses_added_hunk_lines() {
        let patch = "@@ -2,0 +3,2 @@\n+a\n+b\n@@ -10 +12,0 @@\n";
        assert_eq!(parse_new_lines(patch), vec![3, 4]);
    }

    #[test]
    fn diff_scope_keeps_the_new_function_only() {
        let dir = std::env::temp_dir().join(format!("sc-diff-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn old() -> i32 { 1 }\n").unwrap();
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .env("GIT_AUTHOR_NAME", "sc")
                .env("GIT_AUTHOR_EMAIL", "sc@example.com")
                .env("GIT_COMMITTER_NAME", "sc")
                .env("GIT_COMMITTER_EMAIL", "sc@example.com")
                .status()
                .unwrap();
            assert!(status.success(), "{args:?}");
        };
        git(&["init"]);
        git(&["add", "src/lib.rs"]);
        git(&["commit", "-m", "old"]);
        std::fs::write(
            dir.join("src/lib.rs"),
            "pub fn old() -> i32 { 1 }\npub fn added() -> i32 { 2 }\n",
        )
        .unwrap();
        let selection = select(&dir, &[], Some("HEAD"), None, &[]).unwrap();
        assert_eq!(selection.mode, "diff");
        let symbols: Vec<_> = selection
            .crap_functions
            .iter()
            .map(|function| function.symbol.as_str())
            .collect();
        assert_eq!(symbols, vec!["added"]);
        assert!(selection
            .new_symbols
            .contains(&("src/lib.rs".into(), "added".into())));
        assert!(!selection.new_symbols.iter().any(|(_, name)| name == "old"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn overlap_uses_the_function_span() {
        let function = FunctionInfo {
            file: "src/lib.rs".into(),
            symbol: "two".into(),
            span: Span {
                start_line: 4,
                start_col: 1,
                end_line: 6,
                end_col: 2,
            },
            cc: 1,
        };
        assert!(overlaps(&function, &[5]));
        assert!(!overlaps(&function, &[1, 2]));
    }
}

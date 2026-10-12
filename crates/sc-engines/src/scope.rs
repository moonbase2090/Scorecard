// SPDX-License-Identifier: MPL-2.0
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use sc_graph::FunctionInfo;

use crate::command::{run_cmd, CommandError};
use crate::facts::{analyze_rels, AnalyzedFile};

#[derive(Debug, Clone, Copy)]
pub struct ScanScope<'a> {
    pub exclude: &'a [String],
    pub include_generated: &'a [String],
}

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
    /// Tree source paths that are not in this diff, only in diff mode.
    pub other_paths: Option<u64>,
    pub workspace_root: bool,
}

pub fn empty_selection() -> Selection {
    tree_like("tree", Vec::new(), false)
}

#[cfg(test)]
pub fn select(
    root: &Path,
    exclude: &[String],
    diff_base: Option<&str>,
    diff_head: Option<&str>,
    path_list: &[String],
    toolchain_pin: &str,
    perf_enabled: bool,
) -> Result<Selection, String> {
    select_with_generated(
        root,
        ScanScope {
            exclude,
            include_generated: &[],
        },
        diff_base,
        diff_head,
        path_list,
        toolchain_pin,
        perf_enabled,
    )
}

pub fn select_with_generated(
    root: &Path,
    scope: ScanScope<'_>,
    diff_base: Option<&str>,
    diff_head: Option<&str>,
    path_list: &[String],
    toolchain_pin: &str,
    perf_enabled: bool,
) -> Result<Selection, String> {
    if diff_base.is_some() && !path_list.is_empty() {
        return Err("pass either --diff or --paths, not both".into());
    }
    if let Some(base) = diff_base {
        return select_diff(
            root,
            scope,
            base,
            diff_head,
            crate::facts::is_workspace_root(root, toolchain_pin),
            toolchain_pin,
            perf_enabled,
        );
    }
    if !path_list.is_empty() {
        return Ok(select_paths(
            root,
            path_list,
            scope,
            crate::facts::is_workspace_root(root, toolchain_pin),
            perf_enabled,
            toolchain_pin,
        ));
    }
    let (files, workspace_root) = crate::facts::analyze_tree_with_generated(
        root,
        scope.exclude,
        scope.include_generated,
        toolchain_pin,
        perf_enabled,
    );
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
        other_paths: None,
        workspace_root,
    }
}

fn select_paths(
    root: &Path,
    path_list: &[String],
    scope: ScanScope<'_>,
    workspace_root: bool,
    perf_enabled: bool,
    toolchain_pin: &str,
) -> Selection {
    let requested: Vec<String> = path_list
        .iter()
        .map(|path| normalize_rel(root, path))
        .filter(|rel| rel.ends_with(".rs"))
        .collect();
    let rels = sc_graph::filter_paths(root, &requested, scope.exclude, scope.include_generated);
    tree_like(
        "paths",
        analyze_rels(root, &rels, perf_enabled, scope.exclude, toolchain_pin),
        workspace_root,
    )
}

fn select_diff(
    root: &Path,
    scope: ScanScope<'_>,
    base: &str,
    head: Option<&str>,
    workspace_root: bool,
    toolchain_pin: &str,
    perf_enabled: bool,
) -> Result<Selection, String> {
    let base = resolve_base(root, base)?;
    let deltas = diff_files(root, &base, head)?;
    let requested: Vec<String> = deltas
        .iter()
        .filter(|delta| delta.rel.contains("src/"))
        .map(|delta| delta.rel.clone())
        .collect();
    let eligible = sc_graph::filter_paths(root, &requested, scope.exclude, scope.include_generated);
    let deltas: Vec<_> = deltas
        .into_iter()
        .filter(|delta| eligible.contains(&delta.rel))
        .collect();
    let rels: Vec<String> = deltas.iter().map(|delta| delta.rel.clone()).collect();
    let files = analyze_rels(root, &rels, perf_enabled, scope.exclude, toolchain_pin);
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
    let paths: Vec<String> = files.iter().map(|file| file.rel.clone()).collect();
    let (tree_rels, _) = crate::facts::tree_source_rels_with_generated(
        root,
        scope.exclude,
        scope.include_generated,
        toolchain_pin,
    );
    // Diff paths can include src/ files outside the cargo tree scan (deleted
    // members, non-member crates). Count tree paths that are not in the diff,
    // not `tree_len - diff_len`.
    let path_set: std::collections::BTreeSet<&str> = paths.iter().map(String::as_str).collect();
    let other_paths = Some(
        tree_rels
            .iter()
            .filter(|rel| !path_set.contains(rel.as_str()))
            .count() as u64,
    );
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
        other_paths,
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
        if git_ok(root, &["rev-parse", "--verify", "--quiet", requested]) {
            return Ok(requested.to_string());
        }
        return Err(format!(
            "--diff {requested}: ref not found in this checkout. \
             In GitHub Actions set fetch-depth: 0 on actions/checkout, \
             or run git fetch origin main."
        ));
    }
    let head = git(root, &["rev-parse", "HEAD"])
        .map(|sha| sha.trim().to_string())
        .unwrap_or_default();
    for candidate in ["HEAD~1", "main", "master"] {
        let Ok(sha) = git(root, &["rev-parse", "--verify", "--quiet", candidate]) else {
            continue;
        };
        // A branch name that is HEAD (depth-1 clone of main or master, or the
        // only commit on that branch) diffs the commit against itself.
        if sha.trim() == head {
            continue;
        }
        return Ok(candidate.to_string());
    }
    // No history to diff against: HEAD would score zero paths and let the
    // gates pass silently, so fail loudly instead. Explicit --diff HEAD
    // still works for pre-commit flows.
    let shallow = git(root, &["rev-parse", "--is-shallow-repository"])
        .map(|output| output.trim() == "true")
        .unwrap_or(false);
    let mut message = String::from(
        "cannot resolve a diff base automatically (tried HEAD~1, main, master). \
         Fetch full history (clone with fetch-depth: 0, or git fetch --unshallow) \
         or pass an explicit base with --diff BASE.",
    );
    if shallow {
        message.push_str(" This looks like a shallow clone.");
    }
    Err(message)
}

/// Every changed path relative to `root`, including untracked files when
/// `head` is the worktree. Extensions are not filtered; Rust keeps `.rs`
/// in `diff_files`.
pub fn changed_rels(root: &Path, base: &str, head: Option<&str>) -> Result<Vec<String>, String> {
    let names = git_diff_names(root, base, head)?;
    let untracked = if head.is_none() {
        git(root, &["ls-files", "--others", "--exclude-standard"]).unwrap_or_default()
    } else {
        String::new()
    };
    let mut rels = BTreeSet::new();
    for line in names.lines().chain(untracked.lines()) {
        let rel = line.trim().replace('\\', "/");
        if !rel.is_empty() {
            rels.insert(rel);
        }
    }
    Ok(rels.into_iter().collect())
}

/// Added lines in `rels` between `base` and `head` (or the worktree).
/// Untracked paths count every line in the file.
pub fn added_lines(root: &Path, base: &str, head: Option<&str>, rels: &BTreeSet<String>) -> u64 {
    let mut args = vec!["diff", "--numstat", "--relative", base];
    if let Some(head) = head {
        args.push(head);
    }
    let text = git(root, &args).unwrap_or_default();
    let mut total = 0u64;
    let mut seen = BTreeSet::new();
    for line in text.lines() {
        let mut parts = line.split('\t');
        let Some(added) = parts.next() else {
            continue;
        };
        let _deleted = parts.next();
        let Some(path) = parts.next() else {
            continue;
        };
        let path = path.trim().replace('\\', "/");
        if !rels.contains(&path) {
            continue;
        }
        seen.insert(path);
        if added == "-" {
            continue;
        }
        total += added.parse::<u64>().unwrap_or(0);
    }
    for rel in rels {
        if seen.contains(rel) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        total += text.lines().count() as u64;
    }
    total
}

pub fn diff_files(root: &Path, base: &str, head: Option<&str>) -> Result<Vec<FileDelta>, String> {
    let prefix = repo_prefix(root)?;
    let rels = changed_rels(root, base, head)?;
    let mut out = Vec::new();
    for rel in rels {
        if !rel.ends_with(".rs") {
            continue;
        }
        let patch = git_diff_patch(root, base, head, &rel).unwrap_or_default();
        let git_rel = format!("{prefix}{rel}");
        let tracked = git_ok(root, &["cat-file", "-e", &format!("{base}:{git_rel}")]);
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
    let prefix = repo_prefix(root).unwrap_or_default();
    let git_rel = format!("{prefix}{rel}");
    let text = match git(root, &["show", &format!("{base}:{git_rel}")]) {
        Ok(text) => text,
        Err(_) => return BTreeSet::new(),
    };
    sc_graph::function_symbols(&text, rel).into_iter().collect()
}

/// Prefix of `root` inside its git work tree, with a trailing slash.
/// Empty when the project is the repository root.
fn repo_prefix(root: &Path) -> Result<String, String> {
    let prefix = git(root, &["rev-parse", "--show-prefix"])?;
    Ok(prefix.trim().replace('\\', "/"))
}

fn git_diff_names(root: &Path, base: &str, head: Option<&str>) -> Result<String, String> {
    // `--relative` makes names relative to `root` and drops files outside it.
    // `git -C sub diff --name-only` otherwise prints `sub/src/a.rs`.
    let mut args = vec![
        "diff",
        "--relative",
        "--name-only",
        "--diff-filter=ACMR",
        base,
    ];
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

    fn git_here(dir: &std::path::Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "sc")
            .env("GIT_AUTHOR_EMAIL", "sc@example.com")
            .env("GIT_COMMITTER_NAME", "sc")
            .env("GIT_COMMITTER_EMAIL", "sc@example.com")
            .status()
            .unwrap();
        assert!(status.success(), "{args:?}");
    }

    fn single_commit_repo(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn old() -> i32 { 1 }\n").unwrap();
        git_here(&dir, &["init"]);
        git_here(&dir, &["add", "src/lib.rs"]);
        git_here(&dir, &["commit", "-m", "old"]);
        // A feature branch like CI's depth-1 checkout: main/master must not
        // resolve, so AUTO has genuinely nothing to fall back to.
        git_here(&dir, &["branch", "-m", "feat"]);
        dir
    }

    #[test]
    fn auto_base_errors_in_a_single_commit_checkout() {
        let dir = single_commit_repo("diff-base");
        let err = resolve_base(&dir, "AUTO")
            .expect_err("AUTO must not silently fall back to HEAD with no history");
        assert!(
            err.contains("--diff BASE"),
            "message must name the fix: {err}"
        );
        assert!(
            err.contains("fetch-depth: 0"),
            "message must name the CI fix: {err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn auto_base_names_shallow_clones() {
        let src = single_commit_repo("diff-base-src");
        let dst = std::env::temp_dir().join(format!("sc-diff-base-shallow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dst);
        let status = std::process::Command::new("git")
            .args(["clone", "--depth", "1"])
            .arg(&src)
            .arg(&dst)
            .env("GIT_AUTHOR_NAME", "sc")
            .env("GIT_AUTHOR_EMAIL", "sc@example.com")
            .env("GIT_COMMITTER_NAME", "sc")
            .env("GIT_COMMITTER_EMAIL", "sc@example.com")
            .status()
            .unwrap();
        assert!(status.success());
        let err = resolve_base(&dst, "AUTO")
            .expect_err("AUTO must not silently fall back to HEAD in a shallow clone");
        assert!(
            err.contains("shallow"),
            "message must name the shallow clone: {err}"
        );
        let _ = std::fs::remove_dir_all(&dst);
        let _ = std::fs::remove_dir_all(&src);
    }

    #[test]
    fn explicit_head_base_keeps_working() {
        let dir = single_commit_repo("diff-base-head");
        assert_eq!(resolve_base(&dir, "HEAD"), Ok("HEAD".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn auto_base_rejects_a_default_branch_that_is_head() {
        for branch in ["main", "master"] {
            let dir = single_commit_repo(&format!("diff-base-{branch}"));
            git_here(&dir, &["branch", "-M", branch]);
            let err = resolve_base(&dir, "AUTO").expect_err(branch);
            assert!(
                err.contains("fetch-depth: 0"),
                "{branch} must name the fetch-depth fix: {err}"
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn explicit_missing_base_names_the_ref_and_the_fix() {
        let dir = single_commit_repo("diff-base-missing");
        let err = resolve_base(&dir, "does-not-exist").expect_err("missing ref");
        assert!(
            err.contains("--diff does-not-exist:"),
            "message must name the ref: {err}"
        );
        assert!(
            err.contains("fetch-depth: 0"),
            "message must name the fix: {err}"
        );
        assert!(
            !err.contains("fatal:"),
            "message must not be a raw git error: {err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn changed_rels_keeps_python_files() {
        let dir = std::env::temp_dir().join(format!("sc-diff-py-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/old.py"), "def old():\n    return 1\n").unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn old() -> i32 { 1 }\n").unwrap();
        git_here(&dir, &["init"]);
        git_here(&dir, &["add", "."]);
        git_here(&dir, &["commit", "-m", "base"]);
        std::fs::write(dir.join("src/new.py"), "def new():\n    return 2\n").unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn old() -> i32 { 2 }\n").unwrap();
        let rels = changed_rels(&dir, "HEAD", None).unwrap();
        assert!(rels.iter().any(|rel| rel == "src/new.py"), "{rels:?}");
        assert!(rels.iter().any(|rel| rel == "src/lib.rs"), "{rels:?}");
        assert!(!rels.iter().any(|rel| rel == "src/old.py"), "{rels:?}");
        let py: BTreeSet<String> = rels
            .into_iter()
            .filter(|rel| rel.ends_with(".py"))
            .collect();
        assert_eq!(added_lines(&dir, "HEAD", None, &py), 2, "{py:?}");
        let _ = std::fs::remove_dir_all(&dir);
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
        let selection = select(&dir, &[], Some("HEAD"), None, &[], "", false).unwrap();
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
    fn diff_in_a_subdirectory_scores_the_project_path() {
        let dir = std::env::temp_dir().join(format!("sc-diff-sub-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let sub = dir.join("sub");
        std::fs::create_dir_all(sub.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("other/src")).unwrap();
        std::fs::write(sub.join("src/lib.rs"), "pub mod a;\npub mod b;\n").unwrap();
        std::fs::write(sub.join("src/a.rs"), "pub fn a() -> i32 { 1 }\n").unwrap();
        std::fs::write(sub.join("src/b.rs"), "pub fn b() -> i32 { 1 }\n").unwrap();
        std::fs::write(dir.join("other/src/z.rs"), "pub fn z() -> i32 { 1 }\n").unwrap();
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
        git(&["add", "."]);
        git(&["commit", "-m", "base"]);
        std::fs::write(sub.join("src/a.rs"), "pub fn a() -> i32 { 2 }\n").unwrap();
        std::fs::write(dir.join("other/src/z.rs"), "pub fn z() -> i32 { 2 }\n").unwrap();
        std::fs::write(sub.join("src/c.rs"), "pub fn c() -> i32 { 3 }\n").unwrap();
        let selection = select(&sub, &[], Some("HEAD"), None, &[], "", false).unwrap();
        assert_eq!(
            selection.paths,
            vec!["src/a.rs".to_string(), "src/c.rs".to_string()]
        );
        assert_eq!(selection.other_paths, Some(2));
        let symbols: Vec<_> = selection
            .crap_functions
            .iter()
            .map(|function| function.symbol.as_str())
            .collect();
        assert!(symbols.contains(&"a::a"), "{symbols:?}");
        assert!(symbols.contains(&"c::c"), "{symbols:?}");
        assert!(!symbols
            .iter()
            .any(|name| name.contains("z") || name.contains("::b")));
        assert!(!selection.new_symbols.iter().any(|(_, name)| name == "a::a"));
        assert!(selection
            .new_symbols
            .contains(&("src/c.rs".into(), "c::c".into())));
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
    #[test]
    fn other_paths_counts_tree_paths_not_in_the_diff() {
        let tree = ["src/a.rs", "src/b.rs", "src/lib.rs"];
        let paths = ["src/a.rs", "src/lib.rs", "tools/src/main.rs"];
        let path_set: std::collections::BTreeSet<&str> = paths.iter().copied().collect();
        let other = tree.iter().filter(|rel| !path_set.contains(*rel)).count();
        assert_eq!(other, 1);
    }
}

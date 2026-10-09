// SPDX-License-Identifier: MPL-2.0
//! Diff-scoped heuristics for tests made easier to pass alongside product changes.

use std::path::Path;

use sc_core::{Finding, Span, TestValueWeakening};

use crate::mutation::git_diff;

pub struct WeakeningOutcome {
    pub section: TestValueWeakening,
    pub findings: Vec<Finding>,
    pub ran: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DiffLine {
    Context(String),
    Added(String),
    Removed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Hunk {
    new_start: u32,
    lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileDiff {
    path: String,
    deleted: bool,
    hunks: Vec<Hunk>,
}

pub fn run_weakening(root: &Path, diff_base: Option<&str>) -> WeakeningOutcome {
    let Some(base) = diff_base else {
        return skipped("needs --diff");
    };
    let patch = match git_diff(root, base) {
        Ok(patch) => patch,
        Err(err) => return skipped(&err),
    };
    if patch.trim().is_empty() {
        return skipped("empty diff");
    }
    let files = parse_unified_diff(&patch);
    if !files.iter().any(is_product_rust_path) {
        return skipped("no product Rust changes in diff");
    }
    let mut findings = Vec::new();
    for file in &files {
        if !file_touches_tests(file) {
            continue;
        }
        if file.deleted {
            findings.push(deleted_test_file_finding(&file.path, findings.len()));
            continue;
        }
        for hunk in &file.hunks {
            findings.extend(scan_hunk(&file.path, hunk, findings.len()));
        }
    }
    let section = TestValueWeakening {
        status: "ran".into(),
        findings: findings.len() as u64,
        reason: None,
    };
    WeakeningOutcome {
        section,
        findings,
        ran: true,
    }
}

fn skipped(reason: &str) -> WeakeningOutcome {
    WeakeningOutcome {
        section: TestValueWeakening {
            status: "skipped".into(),
            findings: 0,
            reason: Some(reason.into()),
        },
        findings: Vec::new(),
        ran: false,
    }
}

fn is_product_rust_path(file: &FileDiff) -> bool {
    file.path.ends_with(".rs") && !is_test_rust_path(&file.path)
}

fn is_test_rust_path(path: &str) -> bool {
    path.starts_with("tests/")
        || path.contains("/tests/")
        || path.ends_with("_test.rs")
        || path.contains("/benches/")
}

fn file_touches_tests(file: &FileDiff) -> bool {
    is_test_rust_path(&file.path) || file.hunks.iter().any(|hunk| hunk_looks_like_test(hunk))
}

fn hunk_looks_like_test(hunk: &Hunk) -> bool {
    hunk.lines.iter().any(|line| {
        let text = line_text(line);
        text.contains("#[test]")
            || text.contains("#[tokio::test]")
            || text.contains("mod tests")
            || text.contains("fn test_")
    })
}

fn line_text(line: &DiffLine) -> &str {
    match line {
        DiffLine::Context(text) | DiffLine::Added(text) | DiffLine::Removed(text) => text,
    }
}

fn scan_hunk(path: &str, hunk: &Hunk, finding_index: usize) -> Vec<Finding> {
    let mut out = Vec::new();
    let removed: Vec<&str> = hunk
        .lines
        .iter()
        .filter_map(|line| match line {
            DiffLine::Removed(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    let added: Vec<&str> = hunk
        .lines
        .iter()
        .filter_map(|line| match line {
            DiffLine::Added(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();

    for line in &removed {
        if line.contains("#[test]") || line.contains("fn test_") {
            out.push(weakening_finding(
                path,
                hunk.new_start,
                finding_index + out.len(),
                "test_removed",
                "A test was removed in the same diff as product code changes",
            ));
        }
    }
    for line in &added {
        if line.contains("#[ignore]") {
            out.push(weakening_finding(
                path,
                hunk.new_start,
                finding_index + out.len(),
                "ignore_added",
                "#[ignore] was added to a test in the same diff as product code changes",
            ));
        }
        if line.contains("#[should_panic") {
            out.push(weakening_finding(
                path,
                hunk.new_start,
                finding_index + out.len(),
                "should_panic_added",
                "#[should_panic] was added in the same diff as product code changes",
            ));
        }
    }
    if removed.iter().any(|line| line.contains("assert_eq!"))
        && added
            .iter()
            .any(|line| line.contains("assert!(") && !line.contains("assert_eq!"))
    {
        out.push(weakening_finding(
            path,
            hunk.new_start,
            finding_index + out.len(),
            "assert_loosened",
            "assert_eq! was replaced with assert! in the same diff as product code changes",
        ));
    }
    if removed.iter().any(|line| line.contains("assert_eq!"))
        && added
            .iter()
            .any(|line| line.contains(".contains(") || line.contains("contains(&"))
    {
        out.push(weakening_finding(
            path,
            hunk.new_start,
            finding_index + out.len(),
            "assert_loosened",
            "An exact assert_eq! was replaced with a substring or contains check",
        ));
    }
    out
}

fn deleted_test_file_finding(path: &str, index: usize) -> Finding {
    weakening_finding(
        path,
        1,
        index,
        "test_file_deleted",
        "A Rust test file was deleted in the same diff as product code changes",
    )
}

fn weakening_finding(path: &str, line: u32, index: usize, kind: &str, message: &str) -> Finding {
    Finding {
        id: format!("test_value:weakening:{}:{}", path, index),
        rule: "test_value.weakening".into(),
        engine: "test_value".into(),
        severity: "warning".into(),
        file: path.into(),
        span: Some(Span {
            start_line: line,
            start_col: 1,
            end_line: line,
            end_col: 1,
        }),
        symbol: None,
        message: message.into(),
        evidence: serde_json::json!({
            "kind": kind,
            "verification": "unverified",
        }),
        suggested_action: Some(
            "Confirm the test change is intentional; weakened tests may hide regressions".into(),
        ),
        disposition: String::new(),
    }
}

fn parse_unified_diff(patch: &str) -> Vec<FileDiff> {
    let mut files = Vec::new();
    let mut current: Option<FileDiff> = None;
    let mut current_hunk: Option<Hunk> = None;

    for line in patch.lines() {
        if line.starts_with("diff --git ") {
            if let Some(file) = current.take() {
                files.push(file);
            }
            current = Some(FileDiff {
                path: String::new(),
                deleted: false,
                hunks: Vec::new(),
            });
            current_hunk = None;
            continue;
        }
        let file = match current.as_mut() {
            Some(file) => file,
            None => continue,
        };
        if line.starts_with("+++ ") {
            if line.contains("/dev/null") {
                file.deleted = true;
            } else if let Some(path) =
                strip_diff_path(line.strip_prefix("+++ b/").or_else(|| line.get(4..)))
            {
                if file.path.is_empty() {
                    file.path = path;
                }
            }
            continue;
        }
        if line.starts_with("--- ") {
            if line.contains("/dev/null") {
                // new file; deletion handled on +++
            } else if let Some(path) =
                strip_diff_path(line.strip_prefix("--- a/").or_else(|| line.get(4..)))
            {
                if file.path.is_empty() {
                    file.path = path;
                }
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("@@ ") {
            if let Some(hunk) = current_hunk.take() {
                file.hunks.push(hunk);
            }
            current_hunk = Some(parse_hunk_header(rest));
            continue;
        }
        let hunk = match current_hunk.as_mut() {
            Some(hunk) => hunk,
            None => continue,
        };
        if let Some(text) = line.strip_prefix(' ') {
            hunk.lines.push(DiffLine::Context(text.to_string()));
        } else if let Some(text) = line.strip_prefix('+') {
            hunk.lines.push(DiffLine::Added(text.to_string()));
        } else if let Some(text) = line.strip_prefix('-') {
            hunk.lines.push(DiffLine::Removed(text.to_string()));
        }
    }
    if let Some(hunk) = current_hunk {
        if let Some(file) = current.as_mut() {
            file.hunks.push(hunk);
        }
    }
    if let Some(file) = current {
        files.push(file);
    }
    files
}

fn strip_diff_path(path: Option<&str>) -> Option<String> {
    path.map(|path| path.trim().to_string())
        .filter(|path| !path.is_empty() && path != "/dev/null")
}

fn parse_hunk_header(header: &str) -> Hunk {
    let new_start = header
        .split_whitespace()
        .nth(2)
        .and_then(|part| part.split(',').next())
        .and_then(|num| num.trim_start_matches('+').parse().ok())
        .unwrap_or(1);
    Hunk {
        new_start,
        lines: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASSERT_LOOSENED: &str = r"diff --git a/src/lib.rs b/src/lib.rs
index 111..222 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@
 pub fn add(a: i32, b: i32) -> i32 { a + b }
diff --git a/tests/smoke.rs b/tests/smoke.rs
index 333..444 100644
--- a/tests/smoke.rs
+++ b/tests/smoke.rs
@@ -4,7 +4,7 @@ use good_crate::add;
 #[test]
 fn adds() {
-    assert_eq!(add(1, 2), 3);
+    assert!(add(1, 2) > 0);
 }
";

    const CLEAN_DIFF: &str = r"diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@
-pub fn add(a: i32, b: i32) -> i32 { a + b }
+pub fn add(a: i32, b: i32) -> i32 { a + b + 1 }
diff --git a/tests/smoke.rs b/tests/smoke.rs
--- a/tests/smoke.rs
+++ b/tests/smoke.rs
@@ -4,7 +4,7 @@ use good_crate::add;
 #[test]
 fn adds() {
-    assert_eq!(add(1, 2), 3);
+    assert_eq!(add(1, 2), 4);
 }
";

    #[test]
    fn flags_assert_eq_replaced_with_assert() {
        let files = parse_unified_diff(ASSERT_LOOSENED);
        assert!(files.iter().any(is_product_rust_path));
        let mut findings = Vec::new();
        for file in &files {
            if !file_touches_tests(file) {
                continue;
            }
            for hunk in &file.hunks {
                findings.extend(scan_hunk(&file.path, hunk, findings.len()));
            }
        }
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, "test_value.weakening");
        assert_eq!(findings[0].evidence["kind"], "assert_loosened");
    }

    #[test]
    fn exact_assertion_update_is_not_weakening() {
        let files = parse_unified_diff(CLEAN_DIFF);
        let mut findings = Vec::new();
        for file in &files {
            if !file_touches_tests(file) {
                continue;
            }
            for hunk in &file.hunks {
                findings.extend(scan_hunk(&file.path, hunk, findings.len()));
            }
        }
        assert!(findings.is_empty());
    }

    #[test]
    fn ignore_added_is_flagged_when_product_changes() {
        let patch = r"diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1 +1 @@
-pub fn id() -> u8 { 1 }
+pub fn id() -> u8 { 2 }
diff --git a/tests/smoke.rs b/tests/smoke.rs
--- a/tests/smoke.rs
+++ b/tests/smoke.rs
@@ -1,5 +1,6 @@
+#[ignore]
 #[test]
 fn smoke() {
     assert_eq!(1, 1);
 }
";
        let files = parse_unified_diff(patch);
        let mut findings = Vec::new();
        for file in &files {
            if !file_touches_tests(file) {
                continue;
            }
            for hunk in &file.hunks {
                findings.extend(scan_hunk(&file.path, hunk, findings.len()));
            }
        }
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].evidence["kind"], "ignore_added");
    }

    #[test]
    fn skipped_without_diff_base() {
        let out = run_weakening(Path::new("."), None);
        assert!(!out.ran);
        assert_eq!(out.section.status, "skipped");
    }

    fn fixture_patch(name: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/test_value_weakening/patches")
            .join(name);
        std::fs::read_to_string(path).expect("fixture diff")
    }

    #[test]
    fn testdata_bad_patch_is_weakening() {
        let patch = fixture_patch("assert_loosened.diff");
        let files = parse_unified_diff(&patch);
        assert!(files.iter().any(is_product_rust_path));
        let mut findings = Vec::new();
        for file in &files {
            if !file_touches_tests(file) {
                continue;
            }
            for hunk in &file.hunks {
                findings.extend(scan_hunk(&file.path, hunk, findings.len()));
            }
        }
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn testdata_good_patch_is_clean() {
        let patch = fixture_patch("legitimate_assert_update.diff");
        let files = parse_unified_diff(&patch);
        let mut findings = Vec::new();
        for file in &files {
            if !file_touches_tests(file) {
                continue;
            }
            for hunk in &file.hunks {
                findings.extend(scan_hunk(&file.path, hunk, findings.len()));
            }
        }
        assert!(findings.is_empty());
    }
}

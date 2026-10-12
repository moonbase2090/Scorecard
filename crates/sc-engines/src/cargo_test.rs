// SPDX-License-Identifier: MPL-2.0
use std::path::Path;

use sc_core::{Finding, Span};

use crate::compile::normalize_file;

#[derive(Debug, Clone, PartialEq, Eq)]
struct TestFailure {
    name: String,
    file: Option<String>,
    line: Option<u32>,
    col: Option<u32>,
    detail: String,
}

pub fn test_findings(root: &Path, stdout: &str, stderr: &str, success: bool) -> Vec<Finding> {
    let combined = format!("{stdout}\n{stderr}");
    let mut failures = parse_test_output(&combined);
    if !success && failures.is_empty() {
        if let Some(failure) = parse_rustc_human(&combined) {
            failures.push(failure);
        } else {
            failures.push(TestFailure {
                name: "cargo-test".into(),
                file: None,
                line: None,
                col: None,
                detail: "cargo test failed".into(),
            });
        }
    }
    failures
        .into_iter()
        .map(|failure| finding_from(root, failure))
        .collect()
}

/// Test function names in files that mention a changed symbol.
/// More than eight names means the caller should run the full suite.
pub fn targeted_test_names(root: &Path, symbols: &[String]) -> Vec<String> {
    let needles: Vec<String> = symbols
        .iter()
        .filter_map(|symbol| symbol.rsplit("::").next())
        .filter(|name| name.len() >= 3)
        .map(str::to_string)
        .collect();
    if needles.is_empty() {
        return Vec::new();
    }
    let mut files = Vec::new();
    for dir in ["src", "tests"] {
        collect_rs(&root.join(dir), &mut files);
    }
    let mut names = Vec::new();
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if !needles.iter().any(|needle| word_in(&text, needle)) {
            continue;
        }
        names.extend(test_fn_names(&text));
    }
    names.sort();
    names.dedup();
    if names.len() > 8 {
        Vec::new()
    } else {
        names
    }
}

fn collect_rs(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn word_in(text: &str, word: &str) -> bool {
    text.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .any(|token| token == word)
}

fn test_fn_names(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut names = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed != "#[test]" && !trimmed.starts_with("#[test(") {
            continue;
        }
        for next in lines.iter().skip(index + 1).take(4) {
            let next = next.trim();
            if let Some(rest) = next.strip_prefix("fn ") {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    names.push(name);
                }
                break;
            }
            if !next.is_empty() && !next.starts_with('#') && !next.starts_with("//") {
                break;
            }
        }
    }
    names
}

fn finding_from(root: &Path, failure: TestFailure) -> Finding {
    let panic_at = failure
        .file
        .as_deref()
        .filter(|file| panic_outside_project(root, file));
    let (file, span) = if panic_at.is_some() {
        attribution_for_external_panic(root, &failure)
    } else {
        let file = failure
            .file
            .as_deref()
            .map(|file| normalize_file(root, file))
            .filter(|file| !file.is_empty())
            .unwrap_or_else(|| ".".to_string());
        let span = match (failure.line, failure.col) {
            (Some(line), Some(col)) => Some(Span {
                start_line: line,
                start_col: col,
                end_line: line,
                end_col: col.saturating_add(1),
            }),
            _ => None,
        };
        (file, span)
    };
    let message = if failure.detail.is_empty() {
        format!("test {} failed", failure.name)
    } else {
        format!("test {} failed: {}", failure.name, failure.detail)
    };
    let message = if let Some(location) = panic_at {
        format!("{message} (panicked at {location})")
    } else {
        message
    };
    let name = failure.name;
    let evidence = match panic_at {
        Some(location) => serde_json::json!({ "test": name, "panic_at": location }),
        None => serde_json::json!({ "test": name }),
    };
    Finding {
        id: format!("test:{file}:{name}"),
        rule: "test.failed".into(),
        engine: "tests".into(),
        severity: "error".into(),
        file,
        span,
        symbol: Some(name.clone()),
        message,
        evidence,
        suggested_action: Some("Fix the failing test or the behavior it checks".into()),
        disposition: String::new(),
    }
}

fn panic_outside_project(root: &Path, file: &str) -> bool {
    if file.starts_with("/rustc/") || file.contains(".cargo/registry/") {
        return true;
    }
    let path = Path::new(file);
    if path.is_absolute() {
        return path.strip_prefix(root).is_err();
    }
    false
}

fn attribution_for_external_panic(root: &Path, failure: &TestFailure) -> (String, Option<Span>) {
    if let Some((file, line)) = locate_test_function(root, &failure.name) {
        let span = Some(Span {
            start_line: line,
            start_col: 1,
            end_line: line,
            end_col: 2,
        });
        (file, span)
    } else {
        (".".into(), None)
    }
}

fn locate_test_function(root: &Path, test_name: &str) -> Option<(String, u32)> {
    let fn_name = test_name.rsplit("::").next().unwrap_or(test_name);
    let mut files = Vec::new();
    for dir in ["src", "tests"] {
        collect_rs(&root.join(dir), &mut files);
    }
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(line) = line_of_test_fn(&text, fn_name) {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            return Some((rel, line));
        }
    }
    None
}

fn line_of_test_fn(text: &str, fn_name: &str) -> Option<u32> {
    let lines: Vec<&str> = text.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        if !fn_decl_on_line(line, fn_name) {
            continue;
        }
        let start = index.saturating_sub(4);
        if lines[start..=index]
            .iter()
            .any(|near| line_has_test_attribute(near))
        {
            return Some((index + 1) as u32);
        }
    }
    None
}

fn line_has_test_attribute(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed == "#[test]" || trimmed.starts_with("#[test(") || trimmed.contains("#[test]")
}

fn fn_decl_on_line(line: &str, fn_name: &str) -> bool {
    let trimmed = line.trim();
    let Some(rest) = trimmed.split("fn ").nth(1) else {
        return false;
    };
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    name == fn_name
}

fn parse_test_output(output: &str) -> Vec<TestFailure> {
    let lines: Vec<&str> = output.lines().collect();
    let mut names = Vec::new();
    let mut panics = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if let Some(name) = failed_test_name(line) {
            if !names.iter().any(|existing: &String| existing == &name) {
                names.push(name);
            }
        }
        if let Some((name, file, line_no, col)) = panic_location(line) {
            let detail = lines
                .get(index + 1)
                .map(|line| line.trim())
                .filter(|line| !line.is_empty())
                .unwrap_or("")
                .to_string();
            panics.push((name, file, line_no, col, detail));
        }
    }

    if names.is_empty() {
        return panics
            .into_iter()
            .map(|(name, file, line, col, detail)| TestFailure {
                name,
                file: Some(file),
                line: Some(line),
                col: Some(col),
                detail,
            })
            .collect();
    }

    names
        .into_iter()
        .map(|name| {
            if let Some((_, file, line, col, detail)) = panics.iter().find(|(n, ..)| n == &name) {
                TestFailure {
                    name,
                    file: Some(file.clone()),
                    line: Some(*line),
                    col: Some(*col),
                    detail: detail.clone(),
                }
            } else {
                TestFailure {
                    name,
                    file: None,
                    line: None,
                    col: None,
                    detail: String::new(),
                }
            }
        })
        .collect()
}

fn failed_test_name(line: &str) -> Option<String> {
    let line = line.trim();
    let rest = line.strip_prefix("test ")?;
    if !line.contains("... FAILED") {
        return None;
    }
    let name = rest.split_whitespace().next()?.to_string();
    Some(name)
}

fn panic_location(line: &str) -> Option<(String, String, u32, u32)> {
    let line = line.trim();
    let rest = line.strip_prefix("thread '")?;
    let (name, after) = rest.split_once('\'')?;
    let loc = after.split_once("panicked at ")?.1.trim();
    let (file, line_no, col) = find_file_line_col(loc)?;
    Some((name.to_string(), file, line_no, col))
}

fn find_file_line_col(loc: &str) -> Option<(String, u32, u32)> {
    let loc = loc.trim().trim_end_matches(':').trim();
    let candidate = loc
        .rsplit(',')
        .next()
        .unwrap_or(loc)
        .trim()
        .trim_matches('\'')
        .trim();
    let mut parts = candidate.rsplitn(3, ':');
    let col: u32 = parts.next()?.trim().parse().ok()?;
    let line: u32 = parts.next()?.trim().parse().ok()?;
    let file = parts.next()?.trim().trim_matches('\'').trim();
    if file.is_empty() {
        return None;
    }
    Some((file.to_string(), line, col))
}

fn parse_rustc_human(output: &str) -> Option<TestFailure> {
    let lines: Vec<&str> = output.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if !trimmed.starts_with("error") {
            continue;
        }
        for next in lines.iter().skip(index + 1).take(8) {
            let next = next.trim();
            let Some(rest) = next.strip_prefix("--> ") else {
                continue;
            };
            if let Some((file, line_no, col)) = find_file_line_col(rest) {
                return Some(TestFailure {
                    name: "compile".into(),
                    file: Some(file),
                    line: Some(line_no),
                    col: Some(col),
                    detail: trimmed.to_string(),
                });
            }
        }
        return Some(TestFailure {
            name: "compile".into(),
            file: None,
            line: None,
            col: None,
            detail: trimmed.to_string(),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn finds_the_test_that_mentions_classify() {
        let root =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/crap_tested");
        let names = targeted_test_names(&root, &["classify".into()]);
        assert_eq!(names, vec!["covers_branches".to_string()]);
    }

    #[test]
    fn parses_modern_panic_location() {
        let output = "\
test tests::it_adds ... FAILED

thread 'tests::it_adds' panicked at src/lib.rs:11:9:
assertion `left == right` failed
";
        let failures = parse_test_output(output);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].name, "tests::it_adds");
        assert_eq!(failures[0].file.as_deref(), Some("src/lib.rs"));
        assert_eq!(failures[0].line, Some(11));
        assert_eq!(failures[0].col, Some(9));
        assert!(failures[0].detail.contains("assertion"));
    }

    #[test]
    fn parses_legacy_panic_location() {
        let line = "thread 'tests::it_adds' panicked at 'assertion failed', src/lib.rs:8:9";
        let (name, file, line_no, col) = panic_location(line).unwrap();
        assert_eq!(name, "tests::it_adds");
        assert_eq!(file, "src/lib.rs");
        assert_eq!(line_no, 8);
        assert_eq!(col, 9);
    }

    #[test]
    fn parses_panic_location_when_the_thread_line_includes_an_id() {
        let line = "thread 'loc::zero_step' (12345) panicked at /rustc/abc/library/core/src/iter/adapters/step_by.rs:36:9:";
        let (name, file, line_no, col) = panic_location(line).unwrap();
        assert_eq!(name, "loc::zero_step");
        assert_eq!(file, "/rustc/abc/library/core/src/iter/adapters/step_by.rs");
        assert_eq!(line_no, 36);
        assert_eq!(col, 9);
    }

    #[test]
    fn external_rustc_panic_points_at_the_test_source() {
        let root = std::env::temp_dir().join(format!("sc-test-loc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/lib.rs"),
            "pub fn stepped(n: usize) -> usize { (0..10).step_by(n).count() }\n\
#[cfg(test)]\n\
mod loc {\n\
    #[test]\n\
    fn zero_step() { assert_eq!(super::stepped(0), 10); }\n\
}\n",
        )
        .unwrap();
        let output = "\
test loc::zero_step ... FAILED

thread 'loc::zero_step' panicked at /rustc/abc/library/core/src/iter/adapters/step_by.rs:36:9:
assertion `left == right` failed
";
        let findings = test_findings(&root, output, "", false);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].file, "src/lib.rs");
        assert!(!Path::new(&findings[0].file).is_absolute());
        assert_eq!(
            findings[0].evidence["panic_at"],
            "/rustc/abc/library/core/src/iter/adapters/step_by.rs"
        );
        assert!(findings[0].message.contains("panicked at /rustc/"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn external_registry_panic_points_at_the_test_source() {
        let root = std::env::temp_dir().join(format!("sc-test-reg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("tests")).unwrap();
        std::fs::write(
            root.join("tests/smoke.rs"),
            "#[test]\nfn registry_panic() { panic!(\"boom\"); }\n",
        )
        .unwrap();
        let registry = "/home/runner/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/foo-1.0.0/src/lib.rs";
        let output = format!(
            "\
test registry_panic ... FAILED

thread 'registry_panic' panicked at {registry}:12:5:
boom
"
        );
        let findings = test_findings(&root, &output, "", false);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].file, "tests/smoke.rs");
        assert!(!Path::new(&findings[0].file).is_absolute());
        assert_eq!(findings[0].evidence["panic_at"], registry);
        let _ = std::fs::remove_dir_all(&root);
    }
}

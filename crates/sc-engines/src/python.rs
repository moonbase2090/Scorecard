// SPDX-License-Identifier: MPL-2.0
//! Python pack: compile, pytest, Ruff, and imports checked against pyproject.toml.
//!
//! CRAP uses the same formula as Rust. Line coverage comes from pytest-cov when
//! the test run can produce it. SQLite and vector extensions are covered by pytest.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use sc_core::{Finding, RunRecord};

use crate::command::{brief, run_cmd, CommandError};

pub struct PythonOutcome {
    pub findings: Vec<Finding>,
    pub runs: Vec<RunRecord>,
    pub ran: Vec<String>,
    pub skipped: Vec<String>,
    pub types_pass: bool,
    pub types_reason: String,
    pub tests_enforced: bool,
    pub tests_pass: bool,
    pub tests_reason: String,
    pub lint_pass: bool,
    pub lint_reason: String,
    pub sca_errors: u64,
    pub secret_errors: u64,
    pub crap_max: f64,
    pub crap_over: u64,
    pub crap_untested: u64,
    pub crap_coverage_complete: bool,
    pub crap_worst: Vec<sc_core::CrapFunction>,
}

pub fn run(root: &Path, deadline: Instant, threshold: u32, untested_cc: u32) -> PythonOutcome {
    let mut findings = Vec::new();
    let mut runs = Vec::new();
    let mut ran = Vec::new();
    let mut skipped = vec!["complexity".into(), "mutation".into(), "llm".into()];

    let (types_pass, types_reason) = compile_tree(
        root,
        deadline,
        &mut findings,
        &mut runs,
        &mut ran,
        &mut skipped,
    );
    let (tests_enforced, tests_pass, tests_reason) = run_pytest(
        root,
        deadline,
        &mut findings,
        &mut runs,
        &mut ran,
        &mut skipped,
    );
    let (lint_pass, lint_reason) = run_ruff(
        root,
        deadline,
        &mut findings,
        &mut runs,
        &mut ran,
        &mut skipped,
    );
    let sca_errors = check_imports(root, &mut findings, &mut ran);
    let functions = crate::poly_cc::functions_for_pack(root, "python");
    let coverage = read_coverage(root, &functions);
    let crap = crate::crap::evaluate(
        &functions,
        coverage.as_ref(),
        threshold,
        untested_cc,
        |_| true,
    );
    if !crap.coverage_complete {
        let unmatched = coverage
            .as_ref()
            .map(|data| crate::crap::unmatched_count(&functions, data))
            .unwrap_or(functions.len() as u64);
        findings.push(crate::coverage::missing_finding(&format!(
            "coverage data is missing for {unmatched} analyzed function(s)"
        )));
        if coverage.is_none() {
            skipped.push("coverage".into());
        }
    }
    findings.extend(crap.findings);
    ran.push("crap".into());
    let crap_max = crap.crap_max;
    let crap_over = crap.over;
    let crap_untested = crap.untested;
    let crap_coverage_complete = crap.coverage_complete;
    let crap_worst = crap.worst;
    let secrets = crate::pack::text_secrets(root);
    let secret_errors = secrets
        .iter()
        .filter(|finding| finding.severity == "error")
        .count() as u64;
    if !ran.iter().any(|engine| engine == "secrets") {
        ran.push("secrets".into());
    }
    findings.extend(secrets);

    PythonOutcome {
        findings,
        runs,
        ran,
        skipped,
        types_pass,
        types_reason,
        tests_enforced,
        tests_pass,
        tests_reason,
        lint_pass,
        lint_reason,
        sca_errors,
        secret_errors,
        crap_max,
        crap_over,
        crap_untested,
        crap_coverage_complete,
        crap_worst,
    }
}

fn compile_tree(
    root: &Path,
    deadline: Instant,
    findings: &mut Vec<Finding>,
    runs: &mut Vec<RunRecord>,
    ran: &mut Vec<String>,
    skipped: &mut Vec<String>,
) -> (bool, String) {
    let mut targets = Vec::new();
    for name in ["src", "tests"] {
        if root.join(name).is_dir() {
            targets.push(name.to_string());
        }
    }
    if targets.is_empty() {
        targets.push(".".into());
    }
    let mut command = String::from("python3 -m compileall -q");
    for target in &targets {
        command.push(' ');
        command.push_str(target);
    }
    match shell(root, &command, deadline) {
        Ok(captured) => {
            note(
                runs,
                "compile",
                &command,
                captured.status.code(),
                captured.elapsed,
            );
            if captured.status.success() {
                ran.push("compile".into());
                (true, String::new())
            } else {
                ran.push("compile".into());
                let detail = brief(&format!("{}\n{}", captured.stdout, captured.stderr));
                findings.push(error_finding(
                    "compile:failed",
                    "compile.failed",
                    "compile",
                    if detail.is_empty() {
                        "python compile failed".into()
                    } else {
                        format!("python compile failed: {detail}")
                    },
                    "Fix the syntax error and re-run",
                ));
                (false, "python compile failed".into())
            }
        }
        Err(err) => {
            note(runs, "compile", &command, None, Duration::ZERO);
            skipped.push("compile".into());
            findings.push(unavailable("compile", &err.message("python3")));
            (false, "python3 is not available".into())
        }
    }
}

fn has_tests(root: &Path) -> bool {
    root.join("tests").is_dir()
        || root.join("pyproject.toml").is_file() && pyproject_mentions_pytest(root)
}

fn pyproject_mentions_pytest(root: &Path) -> bool {
    std::fs::read_to_string(root.join("pyproject.toml"))
        .map(|text| text.contains("[tool.pytest"))
        .unwrap_or(false)
}

#[inline(never)]
fn run_pytest(
    root: &Path,
    deadline: Instant,
    findings: &mut Vec<Finding>,
    runs: &mut Vec<RunRecord>,
    ran: &mut Vec<String>,
    skipped: &mut Vec<String>,
) -> (bool, bool, String) {
    if !has_tests(root) {
        skipped.push("tests".into());
        return (false, true, String::new());
    }
    let base = if root.join("uv.lock").is_file() || root.join(".venv").is_dir() {
        "uv run --extra dev --with pytest-cov pytest -q".to_string()
    } else {
        "python3 -m pytest -q".to_string()
    };
    let cov_file = root.join(".sc").join("coverage").join("pytest.json");
    if let Some(parent) = cov_file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::remove_file(&cov_file);
    let command = format!("{base} --cov --cov-report=json:{}", cov_file.display());
    let command = if base.starts_with("uv ") || host_pytest() {
        command
    } else {
        crate::toolchain::force_image(&command)
    };
    let captured = match shell(root, &command, deadline) {
        Ok(captured)
            if !captured.status.success()
                && (tool_missing(&captured.stderr, &captured.stdout)
                    || captured.stderr.contains("unrecognized arguments")
                    || captured.stderr.contains("pytest-cov")) =>
        {
            shell(root, &base, deadline)
        }
        other => other,
    };
    match captured {
        Ok(captured) => {
            note(
                runs,
                "tests",
                &command,
                captured.status.code(),
                captured.elapsed,
            );
            if captured.status.success() {
                ran.push("tests".into());
                (true, true, String::new())
            } else if tool_missing(&captured.stderr, &captured.stdout) {
                skipped.push("tests".into());
                findings.push(unavailable("tests", "pytest is not installed"));
                (true, false, "pytest is not installed".into())
            } else {
                ran.push("tests".into());
                let detail = brief(&format!("{}\n{}", captured.stdout, captured.stderr));
                findings.push(error_finding(
                    "tests:failed",
                    "test.failed",
                    "tests",
                    if detail.is_empty() {
                        "pytest failed".into()
                    } else {
                        format!("pytest failed: {detail}")
                    },
                    "Fix the failing test or the behavior it checks",
                ));
                (true, false, "pytest failed".into())
            }
        }
        Err(err) => {
            note(runs, "tests", &command, None, Duration::ZERO);
            skipped.push("tests".into());
            findings.push(unavailable("tests", &err.message("pytest")));
            (true, false, "pytest did not run".into())
        }
    }
}

fn run_ruff(
    root: &Path,
    deadline: Instant,
    findings: &mut Vec<Finding>,
    runs: &mut Vec<RunRecord>,
    ran: &mut Vec<String>,
    skipped: &mut Vec<String>,
) -> (bool, String) {
    let command = if which("ruff") {
        "ruff check .".to_string()
    } else if which("uv") {
        "uvx ruff check .".to_string()
    } else if crate::toolchain::prepare("ruff check .") != "ruff check ." {
        crate::toolchain::force_image("ruff check .")
    } else {
        skipped.push("lint".into());
        findings.push(unavailable("lint", "ruff is not installed"));
        return (false, "ruff is not installed".into());
    };
    match shell(root, &command, deadline) {
        Ok(captured) => {
            note(
                runs,
                "lint",
                &command,
                captured.status.code(),
                captured.elapsed,
            );
            if captured.status.success() {
                ran.push("lint".into());
                (true, String::new())
            } else if tool_missing(&captured.stderr, &captured.stdout) {
                skipped.push("lint".into());
                findings.push(unavailable("lint", "ruff is not installed"));
                (false, "ruff is not installed".into())
            } else {
                ran.push("lint".into());
                let detail = brief(&format!("{}\n{}", captured.stdout, captured.stderr));
                findings.push(error_finding(
                    "lint:failed",
                    "lint.failed",
                    "lint",
                    if detail.is_empty() {
                        "ruff check failed".into()
                    } else {
                        format!("ruff check failed: {detail}")
                    },
                    "Fix the Ruff findings and re-run",
                ));
                (false, "ruff check failed".into())
            }
        }
        Err(err) => {
            note(runs, "lint", &command, None, Duration::ZERO);
            skipped.push("lint".into());
            findings.push(unavailable("lint", &err.message("ruff")));
            (false, "ruff did not run".into())
        }
    }
}

fn check_imports(root: &Path, findings: &mut Vec<Finding>, ran: &mut Vec<String>) -> u64 {
    let manifest = std::fs::read_to_string(root.join("pyproject.toml")).unwrap_or_default();
    let mut allowed = dependency_modules(&manifest);
    if let Some(name) = project_name(&manifest) {
        allowed.insert(normalize_mod(&name));
    }
    for package in local_packages(root) {
        allowed.insert(package);
    }
    let stdlib = stdlib_modules();
    ran.push("sca".into());
    let mut count = 0u64;
    for path in python_files(root) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        for (module, line) in import_roots(&text) {
            let name = normalize_mod(&module);
            if name.is_empty() || stdlib.contains(&name) || allowed.contains(&name) {
                continue;
            }
            count += 1;
            findings.push(Finding {
                id: format!("sca:{rel}:{name}"),
                rule: "sca.hallucinated_import".into(),
                engine: "sca".into(),
                severity: "warning".into(),
                file: rel.clone(),
                span: Some(sc_core::Span {
                    start_line: line,
                    start_col: 1,
                    end_line: line,
                    end_col: 1,
                }),
                symbol: Some(name.clone()),
                message: format!(
                    "Strongly advised: module `{name}` is imported and is not declared in pyproject.toml"
                ),
                evidence: serde_json::json!({"module": name}),
                suggested_action: Some(
                    "Add the distribution to pyproject.toml or remove the import".into(),
                ),
                disposition: String::new(),
            });
        }
    }
    count
}

fn local_packages(root: &Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for base in [root.join("src"), root.to_path_buf()] {
        let Ok(entries) = std::fs::read_dir(&base) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join("__init__.py").is_file() {
                if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                    names.insert(normalize_mod(name));
                }
            }
        }
    }
    names
}

fn python_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for dir in [root.join("src"), root.join("tests")] {
        collect_py(&dir, 0, &mut out);
    }
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("py") {
                out.push(path);
            }
        }
    }
    out
}

fn collect_py(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    if depth > 6 || out.len() >= 400 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name.starts_with('.') || name == "__pycache__" || name == ".venv" {
            continue;
        }
        if path.is_dir() {
            collect_py(&path, depth + 1, out);
        } else if name.ends_with(".py") {
            out.push(path);
        }
    }
}

pub fn import_roots(text: &str) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('#')
            || trimmed.starts_with("from .")
            || trimmed.starts_with("from __future__")
        {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("import ") {
            for part in rest.split(',') {
                let module = part
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .split('.')
                    .next()
                    .unwrap_or("");
                if !module.is_empty() && module != "*" {
                    out.push((module.to_string(), index as u32 + 1));
                }
            }
        } else if let Some(rest) = trimmed.strip_prefix("from ") {
            let module = rest
                .split_whitespace()
                .next()
                .unwrap_or("")
                .split('.')
                .next()
                .unwrap_or("");
            if !module.is_empty() && module != "__future__" {
                out.push((module.to_string(), index as u32 + 1));
            }
        }
    }
    out
}

fn project_name(text: &str) -> Option<String> {
    let mut in_project = false;
    for line in text.lines() {
        let trimmed = strip_comment(line).trim();
        if trimmed.starts_with('[') {
            in_project = trimmed == "[project]";
            continue;
        }
        if in_project {
            if let Some(name) = table_string(trimmed, "name") {
                return Some(name);
            }
        }
    }
    None
}

fn dependency_modules(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut in_project = false;
    let mut in_array = false;
    let mut in_extra = false;
    for line in text.lines() {
        let trimmed = strip_comment(line).trim();
        if trimmed.starts_with('[') {
            in_project = trimmed == "[project]";
            in_extra = trimmed.contains("optional-dependencies")
                || trimmed.starts_with("[dependency-groups");
            in_array = false;
            continue;
        }
        if in_project && (trimmed.starts_with("dependencies") || in_array) {
            if trimmed.starts_with("dependencies") && trimmed.contains('[') && trimmed.contains(']')
            {
                collect_quoted(trimmed, &mut names);
                continue;
            }
            if trimmed.starts_with("dependencies") && trimmed.contains('[') {
                in_array = true;
            }
            if in_array {
                collect_quoted(trimmed, &mut names);
                if trimmed.contains(']') {
                    in_array = false;
                }
            }
            continue;
        }
        if in_extra {
            collect_quoted(trimmed, &mut names);
        }
    }
    names
}

fn collect_quoted(line: &str, names: &mut BTreeSet<String>) {
    let mut rest = line;
    while let Some(start) = rest.find('"').or_else(|| rest.find('\'')) {
        let quote = rest.as_bytes()[start] as char;
        rest = &rest[start + 1..];
        let Some(end) = rest.find(quote) else {
            break;
        };
        let token = &rest[..end];
        let name = token
            .split(['>', '<', '=', '!', '~', '[', ';', ' '])
            .next()
            .unwrap_or("");
        if !name.is_empty() {
            names.insert(normalize_mod(name));
        }
        rest = &rest[end + 1..];
    }
}

fn table_string(line: &str, key: &str) -> Option<String> {
    let (left, right) = line.split_once('=')?;
    if left.trim() != key {
        return None;
    }
    let value = right.trim().trim_matches('"').trim_matches('\'');
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn strip_comment(line: &str) -> &str {
    match line.find('#') {
        Some(index) => &line[..index],
        None => line,
    }
}

fn normalize_mod(name: &str) -> String {
    name.trim().replace('-', "_").to_ascii_lowercase()
}

fn stdlib_modules() -> BTreeSet<String> {
    let mut names = BTreeSet::from([
        "sys".into(),
        "os".into(),
        "re".into(),
        "json".into(),
        "pathlib".into(),
        "typing".into(),
        "collections".into(),
        "dataclasses".into(),
        "enum".into(),
        "sqlite3".into(),
        "hashlib".into(),
        "time".into(),
        "base64".into(),
        "argparse".into(),
        "subprocess".into(),
        "threading".into(),
        "socket".into(),
        "struct".into(),
        "fcntl".into(),
        "hmac".into(),
        "http".into(),
        "urllib".into(),
        "importlib".into(),
        "contextlib".into(),
        "io".into(),
        "types".into(),
        "unittest".into(),
    ]);
    if let Ok(output) = Command::new("python3")
        .args([
            "-c",
            "import sys; print('\\n'.join(sorted(sys.stdlib_module_names)))",
        ])
        .output()
    {
        if output.status.success() {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                if !line.is_empty() {
                    names.insert(line.trim().to_string());
                }
            }
        }
    }
    names
}

fn which(name: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {name} >/dev/null")])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn tool_missing(stderr: &str, stdout: &str) -> bool {
    let text = format!("{stderr}\n{stdout}").to_ascii_lowercase();
    text.contains("no such command")
        || text.contains("not found")
        || text.contains("no module named")
}

fn host_pytest() -> bool {
    Command::new("python3")
        .args(["-c", "import pytest"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn shell(
    root: &Path,
    script: &str,
    deadline: Instant,
) -> Result<crate::command::Captured, CommandError> {
    let script = crate::toolchain::prepare(script);
    let timeout = crate::command::budget_left(deadline)?;
    let mut cmd = Command::new("sh");
    crate::toolchain::apply_docker_host(&mut cmd);
    cmd.current_dir(root)
        .arg("-c")
        .arg(script)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .env("CAIRN_EMBEDD", "0");
    run_cmd(&mut cmd, timeout)
}

#[inline(never)]
fn read_coverage(
    root: &Path,
    functions: &[sc_graph::FunctionInfo],
) -> Option<crate::coverage::CoverageData> {
    let path = root.join(".sc").join("coverage").join("pytest.json");
    if !path.is_file() {
        return None;
    }
    let text = std::fs::read_to_string(&path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let files = value.get("files")?.as_object()?;
    let mut covered = Vec::new();
    for function in functions {
        let Some((_, file)) = files.iter().find(|(name, _)| {
            let name = name.replace('\\', "/");
            name == function.file || name.ends_with(&format!("/{}", function.file))
        }) else {
            continue;
        };
        let executed = line_set(file, "executed_lines");
        let missing = line_set(file, "missing_lines");
        let start = function.span.start_line;
        let end = function.span.end_line.max(start);
        let mut hit = 0u32;
        let mut miss = 0u32;
        for line in start..=end {
            if executed.contains(&line) {
                hit += 1;
            } else if missing.contains(&line) {
                miss += 1;
            }
        }
        if hit + miss == 0 {
            continue;
        }
        let coverage = f64::from(hit) / f64::from(hit + miss);
        covered.push(crate::coverage::CovFunction {
            file: function.file.clone(),
            demangled: function.symbol.clone(),
            coverage,
        });
    }
    let line_rate = value
        .pointer("/totals/percent_covered")
        .and_then(|value| value.as_f64())
        .unwrap_or(0.0)
        / 100.0;
    Some(crate::coverage::CoverageData {
        functions: covered,
        line_rate,
    })
}

fn line_set(file: &serde_json::Value, key: &str) -> BTreeSet<u32> {
    file.get(key)
        .and_then(|value| value.as_array())
        .map(|lines| {
            lines
                .iter()
                .filter_map(|line| line.as_u64().map(|n| n as u32))
                .collect()
        })
        .unwrap_or_default()
}

fn note(
    runs: &mut Vec<RunRecord>,
    engine: &str,
    command: &str,
    exit_code: Option<i32>,
    elapsed: Duration,
) {
    runs.push(RunRecord {
        engine: engine.to_string(),
        command: command.to_string(),
        exit_code,
        duration_ms: u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
    });
}

fn unavailable(engine: &str, message: &str) -> Finding {
    Finding {
        id: format!("{engine}:unavailable"),
        rule: "engine.unavailable".into(),
        engine: engine.into(),
        severity: "warning".into(),
        file: ".".into(),
        span: None,
        symbol: None,
        message: message.into(),
        evidence: serde_json::json!({}),
        suggested_action: Some("Install the tool and re-run".into()),
        disposition: String::new(),
    }
}

fn error_finding(id: &str, rule: &str, engine: &str, message: String, action: &str) -> Finding {
    Finding {
        id: id.into(),
        rule: rule.into(),
        engine: engine.into(),
        severity: "error".into(),
        file: ".".into(),
        span: None,
        symbol: None,
        message,
        evidence: serde_json::json!({}),
        suggested_action: Some(action.into()),
        disposition: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_pytest_json_coverage_for_a_function() {
        let root = std::env::temp_dir().join(format!("sc-py-cov-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".sc/coverage")).unwrap();
        std::fs::write(
            root.join(".sc/coverage/pytest.json"),
            r#"{"files":{"src/app.py":{"executed_lines":[1,2],"missing_lines":[3]}},"totals":{"percent_covered":66.0}}"#,
        )
        .unwrap();
        let functions = vec![sc_graph::FunctionInfo {
            file: "src/app.py".into(),
            symbol: "choose".into(),
            span: sc_core::Span {
                start_line: 1,
                start_col: 1,
                end_line: 3,
                end_col: 1,
            },
            cc: 2,
        }];
        let data = read_coverage(&root, &functions).unwrap();
        assert_eq!(data.functions.len(), 1);
        assert!(data.functions[0].coverage > 0.5);
        assert!(read_coverage(&root.join("missing"), &functions).is_none());
        std::fs::create_dir_all(root.join("tests")).unwrap();
        std::fs::write(
            root.join("tests/test_ok.py"),
            "def test_ok():\n    assert True\n",
        )
        .unwrap();
        let mut findings = Vec::new();
        let mut runs = Vec::new();
        let mut ran = Vec::new();
        let mut skipped = Vec::new();
        let empty = root.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        let _ = run_pytest(
            &empty,
            std::time::Instant::now() + std::time::Duration::from_secs(5),
            &mut findings,
            &mut runs,
            &mut ran,
            &mut skipped,
        );
        let _ = run_pytest(
            &root,
            std::time::Instant::now() + std::time::Duration::from_secs(20),
            &mut findings,
            &mut runs,
            &mut ran,
            &mut skipped,
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn reads_third_party_imports_and_declared_deps() {
        let text = "import numpy as np\nfrom sqlite_vec import serialize_float32\nfrom cairn.store import Vault\n";
        let roots: Vec<_> = import_roots(text)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(roots, vec!["numpy", "sqlite_vec", "cairn"]);
        let manifest = r#"
[project]
name = "cairn"
dependencies = ["numpy>=1.26", "sqlite-vec>=0.1.0"]
[project.optional-dependencies]
dev = ["pytest>=8"]
"#;
        let deps = dependency_modules(manifest);
        assert!(deps.contains("numpy"));
        assert!(deps.contains("sqlite_vec"));
        assert!(deps.contains("pytest"));
        assert_eq!(project_name(manifest).as_deref(), Some("cairn"));
    }

    #[test]
    fn flags_an_undeclared_import() {
        let dir = std::env::temp_dir().join(format!("sc-py-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("pyproject.toml"),
            "[project]\nname = \"demo\"\ndependencies = []\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/app.py"), "import totally_missing\n").unwrap();
        let mut findings = Vec::new();
        let mut ran = Vec::new();
        let count = check_imports(&dir, &mut findings, &mut ran);
        assert_eq!(count, 1);
        assert_eq!(findings[0].rule, "sca.hallucinated_import");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

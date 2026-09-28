// SPDX-License-Identifier: MPL-2.0
//! Python pack: compile, pytest, Ruff, and imports checked against pyproject.toml.
//!
//! A local module is not a finding. An installed or published import missing from
//! pyproject is an undeclared dependency. Hallucinated means the name is not local,
//! not installed, and not on the package index.
//!
//! CRAP uses the same formula as Rust. Line coverage comes from pytest-cov when
//! the test run can produce it. SQLite and vector extensions are covered by pytest.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use sc_core::{Finding, RunRecord};

use crate::command::{brief, run_cmd, CommandError};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScaCounts {
    pub undeclared: u64,
    pub hallucinated: u64,
    pub unresolved: u64,
}

impl ScaCounts {
    pub fn total(self) -> u64 {
        self.undeclared + self.hallucinated + self.unresolved
    }
}

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
    pub sca: ScaCounts,
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
    let functions = crate::poly_cc::functions_for_pack(root, "python");
    let (tests_enforced, tests_pass, tests_reason) = run_pytest(
        root,
        deadline,
        &cov_sources(&functions),
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
    let sca = check_imports(root, &mut findings, &mut ran);
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
        if unmatched > 0 {
            findings.push(crate::coverage::missing_finding(&format!(
                "coverage data is missing for {unmatched} analyzed function(s)"
            )));
        }
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
        sca,
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

pub(crate) const NO_PYTEST_SUITE: &str = "no pytest suite was found. Add a test/ or tests/ directory, a test_*.py or *_test.py file, or a pytest config, then re-run `sc analyze`";

fn has_tests(root: &Path) -> bool {
    root.join("tests").is_dir()
        || root.join("test").is_dir()
        || root_test_file(root)
        || pytest_declared(root)
}

fn root_test_file(root: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(root) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        name.ends_with(".py") && (name.starts_with("test_") || name.ends_with("_test.py"))
    })
}

fn pytest_declared(root: &Path) -> bool {
    pyproject_mentions_pytest(root)
        || root.join("pytest.ini").is_file()
        || ini_has_header(root, "tox.ini", "[pytest]")
        || ini_has_header(root, "setup.cfg", "[tool:pytest]")
}

fn ini_has_header(root: &Path, file: &str, header: &str) -> bool {
    std::fs::read_to_string(root.join(file))
        .map(|text| text.lines().any(|line| line.trim() == header))
        .unwrap_or(false)
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
    sources: &[String],
    findings: &mut Vec<Finding>,
    runs: &mut Vec<RunRecord>,
    ran: &mut Vec<String>,
    skipped: &mut Vec<String>,
) -> (bool, bool, String) {
    if !has_tests(root) {
        skipped.push("tests".into());
        return (false, true, NO_PYTEST_SUITE.into());
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
    let cov: Vec<String> = sources
        .iter()
        .map(|dir| format!("--cov={}", crate::command::shell_quote_arg(dir)))
        .collect();
    let command = format!(
        "{base} {} --cov-report=json:{}",
        cov.join(" "),
        cov_file.display()
    );
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

/// Each directory that holds a scored function, as a `--cov` source. With no
/// source, coverage.py leaves out a file the tests never import, and that file
/// is not scored. A source directory is read even without `__init__.py`, which
/// covers a `src/` layout and namespace packages.
fn cov_sources(functions: &[sc_graph::FunctionInfo]) -> Vec<String> {
    let mut dirs: BTreeSet<String> = functions
        .iter()
        .map(|function| match Path::new(&function.file).parent() {
            Some(dir) if !dir.as_os_str().is_empty() => dir.to_string_lossy().into_owned(),
            _ => ".".into(),
        })
        .collect();
    if dirs.is_empty() {
        dirs.insert(".".into());
    }
    dirs.into_iter().collect()
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

fn check_imports(root: &Path, findings: &mut Vec<Finding>, ran: &mut Vec<String>) -> ScaCounts {
    check_imports_with(root, findings, ran, &LiveIndex::new())
}

fn check_imports_with(
    root: &Path,
    findings: &mut Vec<Finding>,
    ran: &mut Vec<String>,
    index: &dyn PackageIndex,
) -> ScaCounts {
    let manifest = std::fs::read_to_string(root.join("pyproject.toml")).unwrap_or_default();
    let mut allowed = dependency_modules(&manifest);
    if let Some(name) = project_name(&manifest) {
        allowed.insert(normalize_mod(&name));
    }
    for package in local_packages(root) {
        allowed.insert(package);
    }
    let stdlib = stdlib_modules();
    let installed = installed_modules(root);
    ran.push("sca".into());
    let mut counts = ScaCounts::default();
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
            if name.is_empty()
                || stdlib.contains(&name)
                || allowed.contains(&name)
                || is_local(root, &path, &name)
            {
                continue;
            }
            let class = classify(&name, &installed, index);
            record(&mut counts, &class);
            findings.push(import_finding(&rel, &name, line, &class));
        }
    }
    counts
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum IndexHit {
    Present(String),
    Absent,
    Unchecked(String),
}

trait PackageIndex {
    fn lookup(&self, module: &str) -> IndexHit;
}

struct LiveIndex {
    agent: ureq::Agent,
    cache: RefCell<HashMap<String, IndexHit>>,
}

impl LiveIndex {
    fn new() -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(2))
                .user_agent("scorecard (https://github.com/moonbase2090/Scorecard)")
                .build(),
            cache: RefCell::new(HashMap::new()),
        }
    }
}

impl PackageIndex for LiveIndex {
    fn lookup(&self, module: &str) -> IndexHit {
        if let Some(hit) = self.cache.borrow().get(module) {
            return hit.clone();
        }
        let hit = lookup_pypi(&self.agent, "https://pypi.org", module);
        self.cache
            .borrow_mut()
            .insert(module.to_string(), hit.clone());
        hit
    }
}

fn lookup_pypi(agent: &ureq::Agent, origin: &str, module: &str) -> IndexHit {
    let mut absent = false;
    for candidate in index_names(module) {
        match fetch_pypi(agent, origin, &candidate) {
            IndexHit::Present(name) => return IndexHit::Present(name),
            IndexHit::Absent => absent = true,
            IndexHit::Unchecked(reason) => return IndexHit::Unchecked(reason),
        }
    }
    if absent {
        IndexHit::Absent
    } else {
        IndexHit::Unchecked("the package index could not be reached".into())
    }
}

fn fetch_pypi(agent: &ureq::Agent, origin: &str, name: &str) -> IndexHit {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return IndexHit::Absent;
    }
    let url = format!("{origin}/pypi/{name}/json");
    match agent.get(&url).call() {
        Ok(response) => {
            let _ = response.into_string();
            IndexHit::Present(name.to_string())
        }
        Err(ureq::Error::Status(404, response)) => {
            let _ = response.into_string();
            IndexHit::Absent
        }
        Err(ureq::Error::Status(code, response)) => {
            let _ = response.into_string();
            IndexHit::Unchecked(format!("the package index returned HTTP {code}"))
        }
        Err(ureq::Error::Transport(_)) => {
            IndexHit::Unchecked("the package index could not be reached".into())
        }
    }
}

fn index_names(module: &str) -> Vec<String> {
    let normalized = pep503(module);
    if normalized.is_empty() {
        return Vec::new();
    }
    if normalized == module {
        vec![normalized]
    } else {
        vec![normalized, module.to_string()]
    }
}

fn pep503(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.chars() {
        if c == '-' || c == '_' || c == '.' {
            if !out.is_empty() && !dash {
                out.push('-');
                dash = true;
            }
            continue;
        }
        dash = false;
        out.push(c.to_ascii_lowercase());
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

enum Class {
    UndeclaredInstalled,
    UndeclaredPublished(String),
    Hallucinated,
    Unchecked(String),
}

fn classify(name: &str, installed: &BTreeSet<String>, index: &dyn PackageIndex) -> Class {
    if installed.contains(name) {
        return Class::UndeclaredInstalled;
    }
    match index.lookup(name) {
        IndexHit::Present(distribution) => Class::UndeclaredPublished(distribution),
        IndexHit::Absent => Class::Hallucinated,
        IndexHit::Unchecked(reason) => Class::Unchecked(reason),
    }
}

fn record(counts: &mut ScaCounts, class: &Class) {
    match class {
        Class::UndeclaredInstalled | Class::UndeclaredPublished(_) => counts.undeclared += 1,
        Class::Hallucinated => counts.hallucinated += 1,
        Class::Unchecked(_) => counts.unresolved += 1,
    }
}

fn import_finding(rel: &str, name: &str, line: u32, class: &Class) -> Finding {
    let (rule, message, action, resolution, distribution) = match class {
        Class::UndeclaredInstalled => (
            "sca.undeclared_dependency",
            format!(
                "Advisory: module `{name}` is imported and is not declared in pyproject.toml. It is installed in the project environment. Add `{name}` to [project.dependencies] in pyproject.toml."
            ),
            format!("Add `{name}` to [project.dependencies] in pyproject.toml."),
            "installed",
            None,
        ),
        Class::UndeclaredPublished(distribution) => {
            let why = if distribution == name {
                "It is on the package index and is not installed.".to_string()
            } else {
                format!("It is published as `{distribution}` and is not installed.")
            };
            (
                "sca.undeclared_dependency",
                format!(
                    "Advisory: module `{name}` is imported and is not declared in pyproject.toml. {why} Add `{distribution}` to [project.dependencies] in pyproject.toml and install it."
                ),
                format!(
                    "Add `{distribution}` to [project.dependencies] in pyproject.toml and install it."
                ),
                "on_index",
                Some(distribution.clone()),
            )
        }
        Class::Hallucinated => (
            "sca.hallucinated_import",
            format!(
                "Advisory: module `{name}` is imported and does not resolve. It is not a local module, not installed, and not on the package index. Remove the import or correct the module name."
            ),
            format!("Remove the import of `{name}` or correct the module name."),
            "nowhere",
            None,
        ),
        Class::Unchecked(reason) => (
            "sca.import_unresolved",
            format!(
                "Advisory: module `{name}` is imported and is not declared in pyproject.toml. It is not a local module and is not installed. The package index was not checked ({reason}), so this import is not classified as hallucinated. Add the distribution to pyproject.toml or remove the import."
            ),
            format!("Add the distribution that provides `{name}` to pyproject.toml, or remove the import."),
            "index_unchecked",
            None,
        ),
    };
    let mut evidence = serde_json::json!({
        "module": name,
        "resolution": resolution,
    });
    if let Some(distribution) = distribution {
        evidence["distribution"] = serde_json::json!(distribution);
    }
    if let Class::Unchecked(reason) = class {
        evidence["index_error"] = serde_json::json!(reason);
    }
    Finding {
        id: format!("sca:{rel}:{name}"),
        rule: rule.into(),
        engine: "sca".into(),
        severity: "warning".into(),
        file: rel.to_string(),
        span: Some(sc_core::Span {
            start_line: line,
            start_col: 1,
            end_line: line,
            end_col: 1,
        }),
        symbol: Some(name.to_string()),
        message,
        evidence,
        suggested_action: Some(action),
        disposition: String::new(),
    }
}

fn is_local(root: &Path, file: &Path, name: &str) -> bool {
    if name == "conftest" && conftest_above(root, file) {
        return true;
    }
    source_roots(root, file)
        .iter()
        .any(|dir| module_on(dir, name))
}

fn conftest_above(root: &Path, file: &Path) -> bool {
    let mut dir = file.parent();
    while let Some(current) = dir {
        if !current.starts_with(root) {
            break;
        }
        if current.join("conftest.py").is_file() {
            return true;
        }
        if current == root {
            break;
        }
        dir = current.parent();
    }
    false
}

fn source_roots(root: &Path, file: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    roots.push(root.to_path_buf());
    for name in ["src", "tests", "test"] {
        let dir = root.join(name);
        if dir.is_dir() {
            roots.push(dir);
        }
    }
    roots.extend(pytest_pythonpath(root));
    if let Some(parent) = file.parent() {
        if parent.starts_with(root) && import_path_dir(root, parent) {
            roots.push(parent.to_path_buf());
        }
    }
    roots
}

fn import_path_dir(root: &Path, dir: &Path) -> bool {
    if dir.join("__init__.py").is_file() {
        return false;
    }
    let Ok(rel) = dir.strip_prefix(root) else {
        return false;
    };
    matches!(
        rel.components().next().and_then(|c| c.as_os_str().to_str()),
        Some("tests" | "test")
    )
}

fn module_on(dir: &Path, name: &str) -> bool {
    if name.is_empty() || name.contains(['/', '\\']) {
        return false;
    }
    if dir.join(format!("{name}.py")).is_file() {
        return true;
    }
    let pkg = dir.join(name);
    if !pkg.is_dir() {
        return false;
    }
    if pkg.join("__init__.py").is_file() {
        return true;
    }
    let Ok(entries) = std::fs::read_dir(&pkg) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        path.extension().and_then(|ext| ext.to_str()) == Some("py")
            || (path.is_dir() && path.join("__init__.py").is_file())
    })
}

fn pytest_pythonpath(root: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(text) = std::fs::read_to_string(root.join("pyproject.toml")) {
        paths.extend(pythonpath_from_pyproject(&text, root));
    }
    for (file, header) in [
        ("pytest.ini", "[pytest]"),
        ("tox.ini", "[pytest]"),
        ("setup.cfg", "[tool:pytest]"),
    ] {
        if let Ok(text) = std::fs::read_to_string(root.join(file)) {
            paths.extend(pythonpath_from_ini(&text, header, root));
        }
    }
    paths
}

fn pythonpath_from_pyproject(text: &str, root: &Path) -> Vec<PathBuf> {
    let mut in_pytest = false;
    let mut in_array = false;
    let mut paths = Vec::new();
    for line in text.lines() {
        let trimmed = strip_comment(line).trim();
        if trimmed.starts_with('[') {
            in_pytest = trimmed == "[tool.pytest.ini_options]";
            in_array = false;
            continue;
        }
        if !in_pytest {
            continue;
        }
        if !in_array && !trimmed.starts_with("pythonpath") {
            continue;
        }
        if !in_array && trimmed.starts_with("pythonpath") && trimmed.contains('[') {
            in_array = true;
        } else if !in_array {
            if let Some(value) = table_string(trimmed, "pythonpath") {
                push_rel(&mut paths, root, &value);
            }
            continue;
        }
        push_quoted(&mut paths, root, trimmed);
        if trimmed.contains(']') {
            in_array = false;
        }
    }
    paths
}

fn pythonpath_from_ini(text: &str, header: &str, root: &Path) -> Vec<PathBuf> {
    let mut in_section = false;
    let mut collecting = false;
    let mut paths = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_section = trimmed.eq_ignore_ascii_case(header);
            collecting = false;
            continue;
        }
        if !in_section || trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';')
        {
            if trimmed.is_empty() {
                collecting = false;
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("pythonpath") {
            let rest = rest.trim();
            if let Some(value) = rest.strip_prefix('=') {
                let value = value.trim();
                if value.is_empty() {
                    collecting = true;
                } else {
                    push_words(&mut paths, root, value);
                    collecting = false;
                }
            }
            continue;
        }
        if collecting && (line.starts_with(' ') || line.starts_with('\t')) {
            push_words(&mut paths, root, trimmed);
        } else {
            collecting = false;
        }
    }
    paths
}

fn push_quoted(paths: &mut Vec<PathBuf>, root: &Path, line: &str) {
    for token in quoted_tokens(line) {
        push_rel(paths, root, &token);
    }
}

fn push_words(paths: &mut Vec<PathBuf>, root: &Path, line: &str) {
    for part in line.split_whitespace() {
        push_rel(paths, root, part);
    }
}

fn push_rel(paths: &mut Vec<PathBuf>, root: &Path, raw: &str) {
    let raw = raw.trim().trim_matches(['"', '\'']);
    if raw.is_empty() {
        return;
    }
    let path = root.join(raw);
    if path.is_dir() {
        paths.push(path);
    }
}

fn installed_modules(root: &Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for site in site_packages(root) {
        names.append(&mut site_module_names(&site));
    }
    names
}

fn site_module_names(site: &Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let Ok(entries) = std::fs::read_dir(site) else {
        return names;
    };
    for entry in entries.flatten() {
        names.append(&mut entry_module_names(&entry.path()));
    }
    names
}

fn entry_module_names(path: &Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let Some(fname) = path.file_name().and_then(|s| s.to_str()) else {
        return names;
    };
    if fname.ends_with(".dist-info") {
        if let Ok(text) = std::fs::read_to_string(path.join("top_level.txt")) {
            names.extend(top_level_names(&text));
        }
        return names;
    }
    if let Some(stem) = fname.strip_suffix(".py") {
        if !stem.is_empty() {
            names.insert(normalize_mod(stem));
        }
        return names;
    }
    if path.is_dir() && path.join("__init__.py").is_file() {
        names.insert(normalize_mod(fname));
    }
    names
}

fn top_level_names(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for line in text.lines() {
        let name = normalize_mod(line.trim());
        if !name.is_empty() {
            names.insert(name);
        }
    }
    names
}

fn site_packages(root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    push_sites(&root.join(".venv"), &mut dirs);
    push_sites(&root.join("venv"), &mut dirs);
    if let Some(venv) = std::env::var_os("VIRTUAL_ENV") {
        let path = PathBuf::from(venv);
        if path.starts_with(root) {
            push_sites(&path, &mut dirs);
        }
    }
    dirs
}

fn push_sites(venv: &Path, out: &mut Vec<PathBuf>) {
    let lib = venv.join("lib");
    if let Ok(entries) = std::fs::read_dir(&lib) {
        for entry in entries.flatten() {
            let site = entry.path().join("site-packages");
            if site.is_dir() {
                out.push(site);
            }
        }
    }
    let windows = venv.join("Lib").join("site-packages");
    if windows.is_dir() {
        out.push(windows);
    }
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

fn quoted_tokens(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find('"').or_else(|| rest.find('\'')) {
        let quote = rest.as_bytes()[start] as char;
        rest = &rest[start + 1..];
        let Some(end) = rest.find(quote) else {
            break;
        };
        out.push(rest[..end].to_string());
        rest = &rest[end + 1..];
    }
    out
}

fn collect_quoted(line: &str, names: &mut BTreeSet<String>) {
    for token in quoted_tokens(line) {
        let name = token
            .split(['>', '<', '=', '!', '~', '[', ';', ' '])
            .next()
            .unwrap_or("");
        if !name.is_empty() {
            names.insert(normalize_mod(name));
        }
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
        || text.contains("command not found")
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
    let extra = crate::poly_cc::coverage_paths(root, "python");
    let known =
        crate::coverage::merge_known(functions.iter().map(|item| item.file.as_str()), &extra);
    let owners = crate::coverage::file_owners(files.keys().map(|name| name.as_str()), &known);
    let mut covered = Vec::new();
    for function in functions {
        let Some((_, file)) = files
            .iter()
            .find(|(name, _)| crate::coverage::report_owns(&owners, name, &function.file))
        else {
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
        budget_ms: None,
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
    fn a_suite_under_test_runs_and_an_empty_tree_says_so() {
        let root = std::env::temp_dir().join(format!("sc-py-test-dir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("test")).unwrap();
        std::fs::write(root.join("app.py"), "def add(a, b):\n    return a + b\n").unwrap();
        std::fs::write(
            root.join("test/test_app.py"),
            "def test_add():\n    assert False\n",
        )
        .unwrap();
        assert!(has_tests(&root));
        let mut findings = Vec::new();
        let (enforced, pass, _) = run_pytest(
            &root,
            std::time::Instant::now() + std::time::Duration::from_secs(30),
            &[".".to_string()],
            &mut findings,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut Vec::new(),
        );
        assert!(enforced);
        assert!(!pass);
        if host_pytest() {
            assert!(
                findings.iter().any(|finding| finding.rule == "test.failed"),
                "{findings:?}"
            );
        }
        let bare = root.join("bare");
        std::fs::create_dir_all(&bare).unwrap();
        std::fs::write(bare.join("app.py"), "x = 1\n").unwrap();
        assert!(!has_tests(&bare));
        let (enforced, pass, reason) = run_pytest(
            &bare,
            std::time::Instant::now() + std::time::Duration::from_secs(5),
            &[".".to_string()],
            &mut Vec::new(),
            &mut Vec::new(),
            &mut Vec::new(),
            &mut Vec::new(),
        );
        assert!(!enforced);
        assert!(pass);
        assert_eq!(reason, NO_PYTEST_SUITE);
        assert!(!NO_PYTEST_SUITE.contains("not provided"));
        std::fs::write(bare.join("pytest.ini"), "[pytest]\n").unwrap();
        assert!(has_tests(&bare));
        let cfg = root.join("cfg");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(cfg.join("setup.cfg"), "[metadata]\nname = x\n").unwrap();
        assert!(!has_tests(&cfg));
        std::fs::write(cfg.join("setup.cfg"), "[tool:pytest]\n").unwrap();
        assert!(has_tests(&cfg));
        let tox = root.join("tox");
        std::fs::create_dir_all(&tox).unwrap();
        std::fs::write(tox.join("tox.ini"), "[testenv]\n").unwrap();
        assert!(!has_tests(&tox));
        std::fs::write(tox.join("tox.ini"), "[pytest]\n").unwrap();
        assert!(has_tests(&tox));
        let file = root.join("file");
        std::fs::create_dir_all(&file).unwrap();
        std::fs::write(file.join("app_test.py"), "def test_x():\n    assert True\n").unwrap();
        assert!(has_tests(&file));
        let _ = std::fs::remove_dir_all(&root);
    }

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
            &[".".to_string()],
            &mut findings,
            &mut runs,
            &mut ran,
            &mut skipped,
        );
        let _ = run_pytest(
            &root,
            std::time::Instant::now() + std::time::Duration::from_secs(20),
            &[".".to_string()],
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
    fn pep503_normalizes_underscores() {
        assert_eq!(pep503("langchain_core"), "langchain-core");
        assert_eq!(pep503("anyio"), "anyio");
    }

    fn pypi_agent() -> ureq::Agent {
        ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(2))
            .build()
    }

    fn pypi_origin(routes: &[(&str, u16)]) -> String {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let routes: Vec<(String, u16)> = routes
            .iter()
            .map(|(path, status)| ((*path).to_string(), *status))
            .collect();
        std::thread::spawn(move || {
            for _ in 0..routes.len().max(1) {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let mut buf = [0u8; 2048];
                let n = stream.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]);
                let path = request.split(' ').nth(1).unwrap_or("");
                let status = routes
                    .iter()
                    .find(|(route, _)| path.contains(route.as_str()))
                    .map(|(_, status)| *status)
                    .unwrap_or(404);
                let reason = match status {
                    200 => "OK",
                    404 => "Not Found",
                    _ => "Error",
                };
                let body = "{}";
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        format!("http://{addr}")
    }

    #[test]
    fn pypi_lookup_uses_the_normalized_name_then_the_import_name() {
        let agent = pypi_agent();
        assert!(matches!(
            fetch_pypi(&agent, "http://127.0.0.1:1", "not a name"),
            IndexHit::Absent
        ));
        assert!(matches!(
            lookup_pypi(&agent, "http://127.0.0.1:1", "..."),
            IndexHit::Unchecked(_)
        ));
        match lookup_pypi(&agent, "http://127.0.0.1:1", "requests") {
            IndexHit::Unchecked(reason) => {
                assert!(reason.contains("could not be reached"), "{reason}")
            }
            other => panic!("expected an unreachable index, got {other:?}"),
        }

        let origin = pypi_origin(&[("/pypi/foo-bar/json", 404), ("/pypi/Foo_Bar/json", 200)]);
        match lookup_pypi(&agent, &origin, "Foo_Bar") {
            IndexHit::Present(name) => assert_eq!(name, "Foo_Bar"),
            other => panic!("expected the import name, got {other:?}"),
        }
        let missing = pypi_origin(&[("/pypi/nope/json", 404)]);
        assert!(matches!(
            lookup_pypi(&agent, &missing, "nope"),
            IndexHit::Absent
        ));
        let broken = pypi_origin(&[("/pypi/gone/json", 500)]);
        match fetch_pypi(&agent, &broken, "gone") {
            IndexHit::Unchecked(reason) => assert!(reason.contains("HTTP 500"), "{reason}"),
            other => panic!("expected HTTP 500, got {other:?}"),
        }
    }

    #[test]
    fn local_modules_and_pytest_path_are_not_flagged() {
        let dir = scratch("local");
        std::fs::create_dir_all(dir.join("src/demo")).unwrap();
        std::fs::create_dir_all(dir.join("extra")).unwrap();
        std::fs::create_dir_all(dir.join("tests/unit/nested")).unwrap();
        std::fs::create_dir_all(dir.join("tests/e2e/team")).unwrap();
        std::fs::write(
            dir.join("pyproject.toml"),
            "[project]\nname = \"demo\"\ndependencies = [\"numpy>=1.26\"]\n\n[tool.pytest.ini_options]\npythonpath = [\"src\", \"extra\"]\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/demo/__init__.py"), "").unwrap();
        std::fs::write(dir.join("src/demo/requests.py"), "n = 1\n").unwrap();
        std::fs::write(
            dir.join("src/demo/app.py"),
            "import requests\nimport numpy\nimport json\n",
        )
        .unwrap();
        std::fs::write(dir.join("helpers.py"), "n = 1\n").unwrap();
        std::fs::write(dir.join("extra/widgets.py"), "n = 1\n").unwrap();
        std::fs::write(dir.join("tests/unit/conftest.py"), "mark = 1\n").unwrap();
        std::fs::write(
            dir.join("tests/unit/test_app.py"),
            "from conftest import mark\nimport helpers\nimport widgets\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("tests/unit/nested/test_more.py"),
            "from conftest import mark\n",
        )
        .unwrap();
        std::fs::write(dir.join("tests/e2e/common.py"), "n = 1\n").unwrap();
        std::fs::write(dir.join("tests/e2e/agents.py"), "n = 1\n").unwrap();
        std::fs::write(dir.join("tests/e2e/cli_agents.py"), "n = 1\n").unwrap();
        std::fs::write(dir.join("tests/e2e/team/__init__.py"), "name = \"team\"\n").unwrap();
        std::fs::write(
            dir.join("tests/e2e/test_flow.py"),
            "from common import n\nfrom agents import a\nfrom cli_agents import c\nfrom team import name\n",
        )
        .unwrap();
        let (findings, counts) = classify_tree(&dir, IndexHit::Absent);
        let symbols = symbols(&findings);
        assert_eq!(symbols, vec!["requests".to_string()], "{findings:?}");
        assert_eq!(counts.hallucinated, 1);
        assert_eq!(findings[0].rule, "sca.hallucinated_import");
        assert!(findings[0].message.starts_with("Advisory:"));
        assert!(findings[0].message.contains("not on the package index"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn installed_import_is_an_undeclared_dependency() {
        let dir = scratch("installed");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join(".venv/lib/python3.12/site-packages/anyio")).unwrap();
        std::fs::write(
            dir.join("pyproject.toml"),
            "[project]\nname = \"demo\"\ndependencies = []\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/app.py"), "import anyio\n").unwrap();
        std::fs::write(
            dir.join(".venv/lib/python3.12/site-packages/anyio/__init__.py"),
            "",
        )
        .unwrap();
        let (findings, counts) = classify_tree(&dir, IndexHit::Absent);
        assert_eq!(counts.undeclared, 1, "{findings:?}");
        assert_eq!(counts.hallucinated, 0);
        assert_eq!(findings[0].rule, "sca.undeclared_dependency");
        assert!(findings[0].message.starts_with("Advisory:"));
        assert!(findings[0]
            .message
            .contains("installed in the project environment"));
        assert!(!findings[0].message.contains("Strongly"));
        assert!(findings[0]
            .suggested_action
            .as_deref()
            .unwrap()
            .contains("[project.dependencies]"));
        assert_eq!(findings[0].evidence["resolution"], "installed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn published_import_is_an_undeclared_dependency() {
        let dir = scratch("published");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("pyproject.toml"),
            "[project]\nname = \"demo\"\ndependencies = []\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/app.py"), "import langchain_core\n").unwrap();
        let (findings, counts) = classify_tree(&dir, IndexHit::Present("langchain-core".into()));
        assert_eq!(counts.undeclared, 1, "{findings:?}");
        assert_eq!(counts.hallucinated, 0);
        assert_eq!(findings[0].rule, "sca.undeclared_dependency");
        assert!(findings[0]
            .message
            .contains("published as `langchain-core`"));
        assert!(findings[0].message.contains("not installed"));
        assert!(findings[0].message.starts_with("Advisory:"));
        assert_eq!(findings[0].evidence["resolution"], "on_index");
        assert_eq!(findings[0].evidence["distribution"], "langchain-core");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_name_is_hallucinated_only_when_the_index_says_absent() {
        let dir = scratch("missing");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("pyproject.toml"),
            "[project]\nname = \"demo\"\ndependencies = []\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/app.py"), "import not_a_real_module\n").unwrap();
        let (findings, counts) = classify_tree(&dir, IndexHit::Absent);
        assert_eq!(counts.hallucinated, 1);
        assert_eq!(counts.undeclared, 0);
        assert_eq!(findings[0].rule, "sca.hallucinated_import");
        assert!(findings[0].message.starts_with("Advisory:"));
        assert!(findings[0].message.contains("not on the package index"));
        assert!(!findings[0].message.contains("Strongly"));
        assert_eq!(findings[0].evidence["resolution"], "nowhere");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unchecked_index_is_not_called_hallucinated() {
        let dir = scratch("offline");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("pyproject.toml"),
            "[project]\nname = \"demo\"\ndependencies = []\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/app.py"), "import maybe_real\n").unwrap();
        let (findings, counts) = classify_tree(
            &dir,
            IndexHit::Unchecked("the package index could not be reached".into()),
        );
        assert_eq!(counts.unresolved, 1);
        assert_eq!(counts.hallucinated, 0);
        assert_eq!(counts.undeclared, 0);
        assert_eq!(findings[0].rule, "sca.import_unresolved");
        assert!(findings[0].message.starts_with("Advisory:"));
        assert!(findings[0]
            .message
            .contains("not classified as hallucinated"));
        assert!(findings[0]
            .message
            .contains("the package index could not be reached"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cov_sources_names_each_directory_with_a_scored_function() {
        let function = |file: &str| sc_graph::FunctionInfo {
            file: file.into(),
            symbol: "f".into(),
            span: sc_core::Span {
                start_line: 1,
                start_col: 1,
                end_line: 2,
                end_col: 1,
            },
            cc: 1,
        };
        let functions = [
            function("src/app/core.py"),
            function("src/app/unused.py"),
            function("tools/run.py"),
            function("setup.py"),
        ];
        assert_eq!(cov_sources(&functions), [".", "src/app", "tools"]);
        assert_eq!(cov_sources(&[]), ["."]);
    }

    #[test]
    fn pytest_ini_pythonpath_resolves_a_local_module() {
        let dir = scratch("ini");
        write_widgets(&dir);
        std::fs::write(dir.join("pytest.ini"), "[pytest]\npythonpath = extra\n").unwrap();
        let (findings, counts) = classify_tree(&dir, IndexHit::Absent);
        assert_eq!(counts.total(), 0, "{findings:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pyproject_pythonpath_string_resolves_a_local_module() {
        let dir = scratch("pathstr");
        write_widgets(&dir);
        std::fs::write(
            dir.join("pyproject.toml"),
            "[project]\nname = \"demo\"\ndependencies = []\n\n[tool.pytest.ini_options]\npythonpath = \"extra\"\n",
        )
        .unwrap();
        let (findings, counts) = classify_tree(&dir, IndexHit::Absent);
        assert_eq!(counts.total(), 0, "{findings:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    struct FixedIndex(IndexHit);

    impl PackageIndex for FixedIndex {
        fn lookup(&self, _: &str) -> IndexHit {
            self.0.clone()
        }
    }

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sc-py-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_widgets(dir: &Path) {
        std::fs::create_dir_all(dir.join("extra")).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("pyproject.toml"),
            "[project]\nname = \"demo\"\ndependencies = []\n",
        )
        .unwrap();
        std::fs::write(dir.join("extra/widgets.py"), "n = 1\n").unwrap();
        std::fs::write(dir.join("src/app.py"), "import widgets\n").unwrap();
    }

    fn classify_tree(dir: &Path, hit: IndexHit) -> (Vec<Finding>, ScaCounts) {
        let mut findings = Vec::new();
        let counts = check_imports_with(dir, &mut findings, &mut Vec::new(), &FixedIndex(hit));
        (findings, counts)
    }

    fn symbols(findings: &[Finding]) -> Vec<String> {
        findings
            .iter()
            .filter_map(|finding| finding.symbol.clone())
            .collect()
    }
}

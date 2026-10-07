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

pub struct CheckOutcome {
    pub pass: bool,
    pub reason: String,
}

pub struct TestCheck {
    pub enforced: bool,
    pub pass: bool,
    pub reason: String,
}

pub struct PythonChecks {
    pub types: CheckOutcome,
    pub tests: TestCheck,
    pub lint: CheckOutcome,
    pub sca: ScaCounts,
    pub secret_errors: u64,
}

pub struct PythonCrap {
    pub coverage_complete: bool,
    pub worst: Vec<sc_core::CrapFunction>,
    pub crap_max: f64,
    pub over: u64,
    pub untested: u64,
}

pub struct PythonOutcome {
    pub findings: Vec<Finding>,
    pub runs: Vec<RunRecord>,
    pub ran: Vec<String>,
    pub skipped: Vec<String>,
    pub checks: PythonChecks,
    pub crap: PythonCrap,
}

pub fn run_with_generated(
    root: &Path,
    deadline: Instant,
    threshold: u32,
    untested_cc: u32,
    started: std::time::SystemTime,
    scope: crate::scope::ScanScope<'_>,
    coverage_enabled: bool,
) -> PythonOutcome {
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
    let scan =
        crate::poly_cc::scan_for_pack(root, "python", scope.exclude, scope.include_generated);
    let functions = scan.functions;
    let (tests_enforced, tests_pass, tests_reason) = run_pytest(
        root,
        deadline,
        &cov_sources(&functions),
        &mut findings,
        &mut runs,
        &mut ran,
        &mut skipped,
        coverage_enabled,
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
    let mut coverage = read_coverage_with_generated(
        root,
        &functions,
        started,
        scope.exclude,
        scope.include_generated,
    );
    let mut coverage_reason: Option<String> = None;
    let mut coverage_fix: Option<&'static str> = None;
    if !coverage_enabled {
        coverage = None;
    } else if coverage.is_none() && tests_enforced && tests_pass {
        if let Err(err) = run_coverage_fallback(root, deadline, &cov_sources(&functions), &mut runs)
        {
            coverage_reason = Some(err);
            coverage_fix = Some(
                "Install coverage.py (`python3 -m pip install coverage`) and re-run `sc analyze`",
            );
        }
        coverage = read_coverage_with_generated(
            root,
            &functions,
            started,
            scope.exclude,
            scope.include_generated,
        );
    }
    if coverage.is_some() {
        if !ran.iter().any(|engine| engine == "coverage") {
            ran.push("coverage".into());
        }
    } else if !skipped.iter().any(|engine| engine == "coverage") {
        skipped.push("coverage".into());
    }
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
        let tests_ran = ran.iter().any(|engine| engine == "tests");
        let test_exit = runs
            .iter()
            .rev()
            .find(|run| run.engine == "tests" && run.exit_code != Some(0))
            .and_then(|run| run.exit_code);
        let (reason, fix) = if !coverage_enabled {
            (
                "coverage is disabled (`engines.coverage = false`), so coverage was not collected"
                    .to_string(),
                "Set `engines.coverage = true` in analyzer.toml and re-run `sc analyze`",
            )
        } else if !tests_enforced {
            (
                "no pytest suite was detected, so coverage was not collected".to_string(),
                "Add pytest tests, then run `sc analyze` again",
            )
        } else if !tests_pass {
            coverage_tests_skip_reason(tests_ran, test_exit, &tests_reason)
        } else if coverage.is_none() {
            (
                coverage_reason.unwrap_or_else(|| {
                    "pytest finished without writing `.sc/coverage/pytest.json`".into()
                }),
                coverage_fix.unwrap_or(
                    "Install pytest-cov (`python3 -m pip install pytest-cov`) or coverage.py (`python3 -m pip install coverage`), then re-run `sc analyze`",
                ),
            )
        } else {
            (
                format!("coverage data is missing for {unmatched} analyzed function(s)"),
                "Run tests with full coverage reporting and verify source paths match the report",
            )
        };
        if unmatched > 0 || coverage.is_none() {
            findings.push(crate::coverage::missing_finding(&reason, fix));
        }
        if coverage.is_none() && !skipped.iter().any(|engine| engine == "coverage") {
            skipped.push("coverage".into());
        }
    }
    findings.extend(crap.findings);
    ran.push("crap".into());
    let secrets =
        crate::pack::text_secrets_with_generated(root, scope.exclude, scope.include_generated);
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
        checks: PythonChecks {
            types: CheckOutcome {
                pass: types_pass,
                reason: types_reason,
            },
            tests: TestCheck {
                enforced: tests_enforced,
                pass: tests_pass,
                reason: tests_reason,
            },
            lint: CheckOutcome {
                pass: lint_pass,
                reason: lint_reason,
            },
            sca,
            secret_errors,
        },
        crap: PythonCrap {
            coverage_complete: crap.coverage_complete,
            worst: crap.worst,
            crap_max: crap.crap_max,
            over: crap.over,
            untested: crap.untested,
        },
    }
}

#[cfg(test)]
pub fn run(
    root: &Path,
    deadline: Instant,
    threshold: u32,
    untested_cc: u32,
    started: std::time::SystemTime,
    exclude: &[String],
    coverage_enabled: bool,
) -> PythonOutcome {
    run_with_generated(
        root,
        deadline,
        threshold,
        untested_cc,
        started,
        crate::scope::ScanScope {
            exclude,
            include_generated: &[],
        },
        coverage_enabled,
    )
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

/// Message when coverage is skipped because tests failed or never ran.
fn coverage_tests_skip_reason(
    tests_ran: bool,
    test_exit: Option<i32>,
    tests_reason: &str,
) -> (String, &'static str) {
    if tests_ran {
        let reason = test_exit
            .map(|code| {
                format!("coverage skipped: tests gate failed (test command exit code {code})")
            })
            .unwrap_or_else(|| format!("coverage skipped: tests gate failed ({tests_reason})"));
        (
            reason,
            "Fix the failing tests and re-run to collect coverage",
        )
    } else {
        (
            format!("tests did not run ({tests_reason}), so coverage was not collected"),
            "Install the test runner, then re-run `sc analyze`",
        )
    }
}

#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn run_pytest(
    root: &Path,
    deadline: Instant,
    sources: &[String],
    findings: &mut Vec<Finding>,
    runs: &mut Vec<RunRecord>,
    ran: &mut Vec<String>,
    skipped: &mut Vec<String>,
    coverage_enabled: bool,
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
    let base_command = pytest_command(&base, sources, &cov_file, false);
    let command = pytest_command(&base, sources, &cov_file, coverage_enabled);
    let command = if base.starts_with("uv ") || host_pytest() {
        command
    } else {
        crate::toolchain::force_image(&command)
    };
    let mut executed = command.clone();
    let captured = match shell(root, &command, deadline) {
        Ok(captured)
            if !captured.status.success()
                && (tool_missing(&captured.stderr, &captured.stdout)
                    || captured.stderr.contains("unrecognized arguments")
                    || captured.stderr.contains("pytest-cov")) =>
        {
            executed = base_command.clone();
            shell(root, &base_command, deadline)
        }
        other => other,
    };
    match captured {
        Ok(captured) => {
            note(
                runs,
                "tests",
                &executed,
                captured.status.code(),
                captured.elapsed,
            );
            if captured.status.success() {
                ran.push("tests".into());
                (true, true, String::new())
            } else if let Some(message) =
                missing_runner(&command, &captured.stderr, &captured.stdout)
            {
                skipped.push("tests".into());
                let mut finding = unavailable("tests", message);
                if message == "uv is not installed" {
                    finding.suggested_action = Some("Install uv and re-run.".into());
                }
                findings.push(finding);
                (true, false, message.into())
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
            note(runs, "tests", &executed, None, Duration::ZERO);
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

/// Put each Python `src/` root first on the test process's import path. A
/// non-editable install can otherwise make pytest exercise a copy in
/// site-packages while coverage measures the checkout's `src/` files.
fn pythonpath_prefix(sources: &[String]) -> String {
    let mut roots = BTreeSet::new();
    for source in sources {
        let mut root = PathBuf::new();
        for component in Path::new(source).components() {
            root.push(component);
            if component.as_os_str() == "src" {
                roots.insert(root.to_string_lossy().into_owned());
                break;
            }
        }
    }
    if roots.is_empty() {
        return String::new();
    }
    let roots = roots.into_iter().collect::<Vec<_>>().join(":");
    format!(
        "PYTHONPATH={}${{PYTHONPATH:+:\"$PYTHONPATH\"}} ",
        crate::command::shell_quote_arg(&roots)
    )
}

fn coverage_source_flags(sources: &[String]) -> String {
    sources
        .iter()
        .map(|dir| format!("--source={}", crate::command::shell_quote_arg(dir)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Build the pytest command. Coverage adds one `--cov` per source and a JSON
/// report. A `src/` layout puts its source roots first on `PYTHONPATH` either
/// way so tests import the checkout that Scorecard measures.
fn pytest_command(
    base: &str,
    sources: &[String],
    cov_file: &Path,
    coverage_enabled: bool,
) -> String {
    let pythonpath = pythonpath_prefix(sources);
    if !coverage_enabled {
        return format!("{pythonpath}{base}");
    }
    let cov: Vec<String> = sources
        .iter()
        .map(|dir| format!("--cov={}", crate::command::shell_quote_arg(dir)))
        .collect();
    format!(
        "{pythonpath}{base} {} --cov-report=json:{}",
        cov.join(" "),
        cov_file.display()
    )
}

/// `uv run` when the project has `uv.lock` or a `.venv` directory.
/// Otherwise `python3 -m coverage`.
fn coverage_fallback_command(root: &Path, cov_file: &Path, sources: &[String]) -> String {
    let pythonpath = pythonpath_prefix(sources);
    let source_flags = coverage_source_flags(sources);
    let output = cov_file.display();
    if root.join("uv.lock").is_file() || root.join(".venv").is_dir() {
        format!(
            "{pythonpath}uv run --extra dev --with coverage coverage run {source_flags} -m pytest -q && uv run --extra dev --with coverage coverage json -o {output}"
        )
    } else {
        format!(
            "{pythonpath}python3 -m coverage run {source_flags} -m pytest -q && python3 -m coverage json -o {output}"
        )
    }
}

#[inline(never)]
fn run_coverage_fallback(
    root: &Path,
    deadline: Instant,
    sources: &[String],
    runs: &mut Vec<RunRecord>,
) -> Result<(), String> {
    let cov_file = root.join(".sc").join("coverage").join("pytest.json");
    if let Some(parent) = cov_file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Match pytest-cov's --cov sources so a never-imported file is still
    // measured at 0% instead of left out of the report (#120 / #79).
    let command = coverage_fallback_command(root, &cov_file, sources);
    let result = shell(root, &command, deadline).map_err(|err| err.message("coverage.py"))?;
    note(
        runs,
        "coverage",
        &command,
        result.status.code(),
        result.elapsed,
    );
    if result.status.success() {
        Ok(())
    } else {
        let detail = brief(&format!(
            "{}
{}",
            result.stdout, result.stderr
        ));
        if detail.is_empty() {
            Err("coverage.py failed".into())
        } else {
            Err(format!("coverage.py failed: {detail}"))
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
    let place = declaration_place(root);
    let mut allowed = dependency_modules(&manifest);
    allowed.extend(declared_elsewhere(root));
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
                || name == "_typeshed"
                || (rel == "setup.py" && name == "setuptools")
                || stdlib.contains(&name)
                || allowed.contains(&name)
                || is_local(root, &path, &name)
            {
                continue;
            }
            let class = classify(&name, &installed, index);
            record(&mut counts, &class);
            findings.push(import_finding(&rel, &name, line, &class, &place));
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

fn import_finding(rel: &str, name: &str, line: u32, class: &Class, place: &str) -> Finding {
    let add = |distribution: &str| -> String {
        if place == "pyproject.toml" {
            format!("Add `{distribution}` to [project.dependencies] in pyproject.toml.")
        } else if place == "setup.py" || place == "setup.cfg" {
            format!("Add `{distribution}` to install_requires in {place}.")
        } else {
            format!("Add `{distribution}` to {place}.")
        }
    };
    let (rule, message, action, resolution, distribution) = match class {
        Class::UndeclaredInstalled => (
            "sca.undeclared_dependency",
            format!(
                "Advisory: module `{name}` is imported and is not declared in {place}. It is installed in the project environment. {}",
                add(name)
            ),
            add(name),
            "installed",
            None,
        ),
        Class::UndeclaredPublished(distribution) => {
            let why = if distribution == name {
                "It is on the package index and is not installed.".to_string()
            } else {
                format!("It is published as `{distribution}` and is not installed.")
            };
            let action = if place == "pyproject.toml" {
                format!("Add `{distribution}` to [project.dependencies] in pyproject.toml and install it.")
            } else {
                format!("{} Install it.", add(distribution))
            };
            (
                "sca.undeclared_dependency",
                format!(
                    "Advisory: module `{name}` is imported and is not declared in {place}. {why} {action}"
                ),
                action,
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
                "Advisory: module `{name}` is imported and is not declared in {place}. It is not a local module and is not installed. The package index was not checked ({reason}), so this import is not classified as hallucinated. {} Or remove the import.",
                add(name)
            ),
            format!("{}, or remove the import.", add(name).trim_end_matches('.')),
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
    let masked = code_only(text);
    let optional = optional_import_lines(&masked);
    let mut out = Vec::new();
    for (index, line) in masked.lines().enumerate() {
        let line_no = index as u32 + 1;
        if optional.contains(&line_no) {
            continue;
        }
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
                if module_name(module) {
                    out.push((module.to_string(), line_no));
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
            if module_name(module) && module != "__future__" {
                out.push((module.to_string(), line_no));
            }
        }
    }
    out
}

fn module_name(name: &str) -> bool {
    !name.is_empty() && name != "*" && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Replace comments and string contents with spaces, keeping newlines, so a
/// docstring cannot look like an import.
fn code_only(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if let Some((quote, triple, raw, skip)) = string_at(&chars[i..]) {
            for _ in 0..skip {
                out.push(if chars[i] == '\n' { '\n' } else { ' ' });
                i += 1;
            }
            if triple {
                let closer = [quote, quote, quote];
                while i < chars.len() {
                    if chars[i..].starts_with(&closer) {
                        out.push_str("   ");
                        i += 3;
                        break;
                    }
                    let c = chars[i];
                    out.push(if c == '\n' { '\n' } else { ' ' });
                    i += 1;
                }
            } else {
                while i < chars.len() {
                    let c = chars[i];
                    if !raw && c == '\\' {
                        out.push(' ');
                        i += 1;
                        if i < chars.len() {
                            out.push(if chars[i] == '\n' { '\n' } else { ' ' });
                            i += 1;
                        }
                        continue;
                    }
                    out.push(if c == '\n' { '\n' } else { ' ' });
                    i += 1;
                    if c == quote || c == '\n' {
                        break;
                    }
                }
            }
            continue;
        }
        if chars[i] == '#' {
            while i < chars.len() && chars[i] != '\n' {
                out.push(' ');
                i += 1;
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn string_at(rest: &[char]) -> Option<(char, bool, bool, usize)> {
    let mut i = 0;
    let mut raw = false;
    while i < rest.len()
        && i < 3
        && matches!(rest[i], 'r' | 'R' | 'b' | 'B' | 'f' | 'F' | 'u' | 'U')
    {
        if matches!(rest[i], 'r' | 'R') {
            raw = true;
        }
        i += 1;
    }
    let quote = *rest.get(i)?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let triple = rest.get(i + 1) == Some(&quote) && rest.get(i + 2) == Some(&quote);
    Some((quote, triple, raw, i + if triple { 3 } else { 1 }))
}

fn optional_import_lines(text: &str) -> BTreeSet<u32> {
    let mut skip = BTreeSet::new();
    let mut stack: Vec<(usize, bool, Vec<u32>)> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let line_no = index as u32 + 1;
        while let Some((try_indent, caught, lines)) = stack.last() {
            let clause = trimmed.starts_with("except")
                || trimmed.starts_with("else:")
                || trimmed.starts_with("finally:");
            if indent < *try_indent || (indent == *try_indent && !clause) {
                let (_, caught, lines) = stack.pop().unwrap();
                if caught {
                    skip.extend(lines);
                }
            } else {
                let _ = (caught, lines);
                break;
            }
        }
        if trimmed == "try:" || trimmed.starts_with("try:") {
            stack.push((indent, false, Vec::new()));
            continue;
        }
        if let Some((try_indent, caught, lines)) = stack.last_mut() {
            if indent == *try_indent
                && trimmed.starts_with("except")
                && (trimmed.contains("ImportError") || trimmed.contains("ModuleNotFoundError"))
            {
                *caught = true;
            }
            if indent > *try_indent
                && (trimmed.starts_with("import ") || trimmed.starts_with("from "))
            {
                lines.push(line_no);
            }
        }
    }
    for (_, caught, lines) in stack {
        if caught {
            skip.extend(lines);
        }
    }
    skip
}

fn declaration_place(root: &Path) -> String {
    if root.join("pyproject.toml").is_file() {
        "pyproject.toml".into()
    } else if root.join("requirements.txt").is_file() {
        "requirements.txt".into()
    } else if root.join("setup.cfg").is_file() {
        "setup.cfg".into()
    } else if root.join("setup.py").is_file() {
        "setup.py".into()
    } else {
        "pyproject.toml".into()
    }
}

fn declared_elsewhere(root: &Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    if let Ok(text) = std::fs::read_to_string(root.join("requirements.txt")) {
        names.extend(requirements_modules(&text));
    }
    if let Ok(text) = std::fs::read_to_string(root.join("setup.cfg")) {
        names.extend(setup_cfg_requires(&text));
    }
    if let Ok(text) = std::fs::read_to_string(root.join("setup.py")) {
        names.extend(setup_py_requires(&text));
    }
    names
}

fn requirements_modules(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for line in text.lines() {
        if let Some(name) = requirement_name(line) {
            names.insert(name);
        }
    }
    names
}

fn requirement_name(line: &str) -> Option<String> {
    let line = strip_comment(line).trim();
    if line.is_empty() || line.starts_with('-') {
        return None;
    }
    let name = line
        .split(['>', '<', '=', '!', '~', '[', ';', ' ', '\\'])
        .next()
        .unwrap_or("");
    if name.is_empty() {
        None
    } else {
        Some(normalize_mod(name))
    }
}

fn setup_cfg_requires(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut in_options = false;
    let mut in_requires = false;
    for line in text.lines() {
        let trimmed = strip_comment(line).trim();
        if trimmed.starts_with('[') {
            in_options = trimmed == "[options]";
            in_requires = false;
            continue;
        }
        if !in_options {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("install_requires") {
            let rest = rest.trim().trim_start_matches('=').trim();
            if let Some(name) = requirement_name(rest) {
                names.insert(name);
            }
            in_requires = true;
            continue;
        }
        if in_requires {
            if !line.starts_with(char::is_whitespace) && !trimmed.is_empty() {
                in_requires = false;
                continue;
            }
            if let Some(name) = requirement_name(trimmed) {
                names.insert(name);
            }
        }
    }
    names
}

fn setup_py_requires(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut rest = text;
    while let Some(index) = rest.find("install_requires") {
        let line_start = rest[..index].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let commented = rest[line_start..index].trim_start().starts_with('#');
        rest = &rest[index + "install_requires".len()..];
        if commented {
            continue;
        }
        let after = rest.trim_start().trim_start_matches('=').trim_start();
        if after.starts_with('[') || after.starts_with('(') {
            let mut depth = 0;
            let mut chunk = String::new();
            for c in after.chars() {
                chunk.push(c);
                if c == '[' || c == '(' {
                    depth += 1;
                }
                if c == ']' || c == ')' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
            collect_quoted(&chunk, &mut names);
        }
    }
    names
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

/// The test command was `uv run ...` and the shell could not find `uv`.
/// A missing pytest inside that command still names pytest.
fn missing_runner(command: &str, stderr: &str, stdout: &str) -> Option<&'static str> {
    if !tool_missing(stderr, stdout) {
        return None;
    }
    let text = format!("{stderr}\n{stdout}").to_ascii_lowercase();
    let uv = command.split_whitespace().next() == Some("uv");
    let uv_missing = text.contains("uv: command not found")
        || text.contains("command not found: uv")
        || text.contains("uv: not found")
        || text.contains("no such command: uv");
    if uv && uv_missing {
        Some("uv is not installed")
    } else {
        Some("pytest is not installed")
    }
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
fn read_coverage_with_generated(
    root: &Path,
    functions: &[sc_graph::FunctionInfo],
    since: std::time::SystemTime,
    exclude: &[String],
    include_generated: &[String],
) -> Option<crate::coverage::CoverageData> {
    let path = root.join(".sc").join("coverage").join("pytest.json");
    if !crate::pack_cov::written_during_run(&path, since) {
        return None;
    }
    let text = std::fs::read_to_string(&path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let files = value.get("files")?.as_object()?;
    let extra =
        crate::poly_cc::coverage_paths_with_scope(root, "python", exclude, include_generated);
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

#[cfg(test)]
fn read_coverage(
    root: &Path,
    functions: &[sc_graph::FunctionInfo],
    since: std::time::SystemTime,
) -> Option<crate::coverage::CoverageData> {
    read_coverage_with_generated(root, functions, since, &[], &[])
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
    fn pytest_command_omits_cov_flags_when_coverage_is_disabled() {
        let cov_file = std::path::PathBuf::from("/tmp/sc-pytest.json");
        let sources = vec![".".to_string(), "src/app".to_string()];
        let off = pytest_command("python3 -m pytest -q", &sources, &cov_file, false);
        assert!(
            off.starts_with("PYTHONPATH='src'${PYTHONPATH:+:\"$PYTHONPATH\"} python3 -m pytest -q"),
            "{off}"
        );
        assert!(!off.contains("--cov"), "{off}");
        assert_eq!(
            pytest_command("python3 -m pytest -q", &[".".into()], &cov_file, false),
            "python3 -m pytest -q"
        );
        let on = pytest_command("python3 -m pytest -q", &sources, &cov_file, true);
        assert!(
            on.starts_with("PYTHONPATH='src'${PYTHONPATH:+:\"$PYTHONPATH\"} python3 -m pytest -q"),
            "{on}"
        );
        assert!(on.contains("--cov='.'"), "{on}");
        assert!(on.contains("--cov='src/app'"), "{on}");
        assert!(on.contains("--cov-report=json:"), "{on}");
    }

    #[test]
    fn cdk_out_assets_add_no_cov_source_and_no_functions() {
        let root = std::env::temp_dir().join(format!("sc-py-cdkout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("aws/infra/cdk.out/asset.123")).unwrap();
        std::fs::create_dir_all(root.join("app")).unwrap();
        std::fs::write(
            root.join("aws/infra/cdk.out/asset.123/synth.py"),
            "def handler(event):\n    return event\n",
        )
        .unwrap();
        std::fs::write(root.join("app/main.py"), "def main():\n    return 0\n").unwrap();
        // Mirror the Cairn layout: the synthesized build output is git-ignored.
        std::fs::write(root.join(".gitignore"), "cdk.out/\n").unwrap();

        let scan = crate::poly_cc::scan_for_pack(&root, "python", &[], &[]);
        assert!(
            !scan.paths.iter().any(|path| path.contains("cdk.out")),
            "{:?}",
            scan.paths
        );
        assert!(
            !scan
                .functions
                .iter()
                .any(|function| function.file.contains("cdk.out")),
            "{:?}",
            scan.functions
        );
        assert!(
            scan.functions
                .iter()
                .any(|function| function.file == "app/main.py"),
            "{:?}",
            scan.functions
        );
        let sources = cov_sources(&scan.functions);
        assert!(
            !sources.iter().any(|source| source.contains("cdk.out")),
            "{sources:?}"
        );
        let command = pytest_command(
            "python3 -m pytest -q",
            &sources,
            &root.join("cov.json"),
            true,
        );
        assert!(!command.contains("cdk.out"), "{command}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn coverage_disabled_skips_collection_and_names_the_flag() {
        let root = std::env::temp_dir().join(format!("sc-py-nocov-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("app.py"), "def wide(n):\n    return n\n").unwrap();
        let outcome = run(
            &root,
            std::time::Instant::now() + std::time::Duration::from_secs(60),
            30,
            15,
            std::time::SystemTime::now(),
            &[],
            false,
        );
        assert!(!outcome.crap.coverage_complete);
        assert!(outcome.crap.worst.is_empty());
        assert!(outcome.skipped.iter().any(|engine| engine == "coverage"));
        let missing = outcome
            .findings
            .iter()
            .find(|finding| finding.rule == "coverage.missing")
            .expect("coverage.missing when disabled");
        assert!(
            missing.message.contains("engines.coverage = false"),
            "{}",
            missing.message
        );
        assert!(
            missing
                .suggested_action
                .as_deref()
                .unwrap_or("")
                .contains("engines.coverage = true"),
            "{missing:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn coverage_tests_skip_reason_does_not_blame_missing_pytest() {
        let (reason, fix) = coverage_tests_skip_reason(false, Some(127), "pytest is not installed");
        assert!(!reason.contains("tests gate failed"), "{reason}");
        assert!(!reason.contains("exit code"), "{reason}");
        assert!(reason.contains("tests did not run"), "{reason}");
        assert!(fix.contains("Install the test runner"), "{fix}");
    }

    #[test]
    fn coverage_tests_skip_reason_names_exit_code_when_tests_ran() {
        let (reason, fix) = coverage_tests_skip_reason(true, Some(1), "pytest failed");
        assert!(reason.contains("tests gate failed"), "{reason}");
        assert!(reason.contains("exit code 1"), "{reason}");
        assert_eq!(fix, "Fix the failing tests and re-run to collect coverage");
    }

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
            true,
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
            true,
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
        let before = crate::pack_cov::run_start() - std::time::Duration::from_secs(60);
        let data = read_coverage(&root, &functions, before).unwrap();
        assert_eq!(data.functions.len(), 1);
        assert!(
            (data.functions[0].coverage - 2.0 / 3.0).abs() < 1e-9,
            "coverage {}",
            data.functions[0].coverage
        );
        assert!(read_coverage(&root.join("missing"), &functions, before).is_none());
        // A report older than this run is left from an earlier one.
        let after = crate::pack_cov::run_start() + std::time::Duration::from_secs(60);
        assert!(read_coverage(&root, &functions, after).is_none());
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
            true,
        );
        let _ = run_pytest(
            &root,
            std::time::Instant::now() + std::time::Duration::from_secs(20),
            &[".".to_string()],
            &mut findings,
            &mut runs,
            &mut ran,
            &mut skipped,
            true,
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_uv_is_not_called_a_missing_pytest() {
        assert_eq!(
            missing_runner(
                "uv run --extra dev --with pytest-cov pytest -q",
                "sh: uv: command not found\n",
                ""
            ),
            Some("uv is not installed")
        );
        assert_eq!(
            missing_runner(
                "python3 -m pytest -q",
                "/usr/bin/python3: No module named pytest\n",
                ""
            ),
            Some("pytest is not installed")
        );
        assert_eq!(
            missing_runner("uv run pytest -q", "pytest failed\n", ""),
            None
        );
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
    fn prose_typeshed_optional_and_requirements_are_not_findings() {
        let prose = "def jar():\n    \"\"\"Take a cookie from the jar.\"\"\"\n    return 1\n";
        assert!(import_roots(prose).is_empty(), "{:?}", import_roots(prose));
        let typeshed = scratch("typeshed");
        std::fs::write(
            typeshed.join("app.py"),
            "from typing import TYPE_CHECKING\nif TYPE_CHECKING:\n    from _typeshed import Incomplete\n",
        )
        .unwrap();
        let (findings, _) = classify_tree(&typeshed, IndexHit::Absent);
        assert!(
            !findings
                .iter()
                .any(|finding| finding.symbol.as_deref() == Some("_typeshed")),
            "{findings:?}"
        );
        let optional =
            "try:\n    import simplejson as json\nexcept ImportError:\n    import json\n";
        let roots: Vec<_> = import_roots(optional)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert!(!roots.iter().any(|name| name == "simplejson"), "{roots:?}");

        let dir = scratch("reqs");
        std::fs::write(dir.join("requirements.txt"), "urllib3>=2\n").unwrap();
        std::fs::write(dir.join("app.py"), "import urllib3\n").unwrap();
        let (findings, _) = classify_tree(&dir, IndexHit::Absent);
        assert!(findings.is_empty(), "{findings:?}");

        let setup = scratch("setup");
        std::fs::write(
            setup.join("setup.py"),
            "import setuptools\nsetuptools.setup(install_requires=[\"urllib3\"])\n",
        )
        .unwrap();
        std::fs::write(setup.join("app.py"), "import urllib3\n").unwrap();
        let (findings, _) = classify_tree(&setup, IndexHit::Absent);
        assert!(
            !findings
                .iter()
                .any(|finding| finding.symbol.as_deref() == Some("setuptools")
                    || finding.symbol.as_deref() == Some("urllib3")),
            "{findings:?}"
        );

        let cfg = scratch("cfg");
        std::fs::write(
            cfg.join("setup.cfg"),
            "[options]\ninstall_requires =\n    urllib3\n",
        )
        .unwrap();
        std::fs::write(cfg.join("app.py"), "import urllib3\n").unwrap();
        let (findings, _) = classify_tree(&cfg, IndexHit::Absent);
        assert!(findings.is_empty(), "{findings:?}");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&setup);
        let _ = std::fs::remove_dir_all(&cfg);
        let _ = std::fs::remove_dir_all(&typeshed);
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
    fn src_layout_tests_import_the_checkout_before_an_installed_copy() {
        let dir = scratch("srcpath");
        let source_package = dir.join("src/sample_pkg");
        let installed_package = dir.join("installed/sample_pkg");
        std::fs::create_dir_all(&source_package).unwrap();
        std::fs::create_dir_all(&installed_package).unwrap();
        std::fs::write(source_package.join("__init__.py"), "ORIGIN = 'checkout'\n").unwrap();
        std::fs::write(
            installed_package.join("__init__.py"),
            "ORIGIN = 'installed'\n",
        )
        .unwrap();

        let import = "import sample_pkg; print(sample_pkg.ORIGIN)";
        let baseline_command = format!("python3 -c {}", crate::command::shell_quote_arg(import));
        let mut baseline = Command::new("sh");
        baseline
            .current_dir(&dir)
            .env("PYTHONPATH", dir.join("installed"))
            .args(["-c", baseline_command.as_str()]);
        let installed = baseline.output().expect("python3 is available");
        assert!(
            installed.status.success(),
            "{}",
            String::from_utf8_lossy(&installed.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&installed.stdout).trim(),
            "installed"
        );

        let command = pytest_command(
            &format!("python3 -c {}", crate::command::shell_quote_arg(import)),
            &["src/sample_pkg".into()],
            &dir.join(".sc/coverage/pytest.json"),
            false,
        );
        let mut with_src = Command::new("sh");
        with_src
            .current_dir(&dir)
            .env("PYTHONPATH", dir.join("installed"))
            .args(["-c", &command]);
        let checkout = with_src.output().expect("python3 is available");
        assert!(
            checkout.status.success(),
            "{}",
            String::from_utf8_lossy(&checkout.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&checkout.stdout).trim(), "checkout");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn coverage_fallback_passes_the_same_sources_as_pytest_cov() {
        let flags = coverage_source_flags(&[".".into(), "src/app".into(), "tools".into()]);
        assert_eq!(flags, "--source='.' --source='src/app' --source='tools'");
        assert!(flags.contains("--source='src/app'"));
    }

    #[test]
    fn coverage_fallback_uses_uv_only_when_the_project_has_a_lock_or_venv() {
        let dir = scratch("cov-cmd");
        let cov_file = dir.join(".sc/coverage/pytest.json");
        let sources = ["src".to_string(), ".".to_string()];
        let python3 = coverage_fallback_command(&dir, &cov_file, &sources);
        assert!(python3.starts_with(
            "PYTHONPATH='src'${PYTHONPATH:+:\"$PYTHONPATH\"} python3 -m coverage run --source='src' --source='.' -m pytest -q && "
        ));
        assert!(python3.contains(&format!(
            "python3 -m coverage json -o {}",
            cov_file.display()
        )));
        assert!(!python3.contains("uv run"));

        std::fs::write(dir.join(".venv"), "not a directory\n").unwrap();
        assert!(coverage_fallback_command(&dir, &cov_file, &sources)
            .starts_with("PYTHONPATH='src'${PYTHONPATH:+:\"$PYTHONPATH\"} python3 -m coverage"));
        std::fs::remove_file(dir.join(".venv")).unwrap();

        std::fs::create_dir(dir.join(".venv")).unwrap();
        let venv = coverage_fallback_command(&dir, &cov_file, &sources);
        assert!(venv.starts_with(
            "PYTHONPATH='src'${PYTHONPATH:+:\"$PYTHONPATH\"} uv run --extra dev --with coverage coverage run --source='src' --source='.' -m pytest -q && "
        ));
        assert!(venv.contains(&format!(
            "uv run --extra dev --with coverage coverage json -o {}",
            cov_file.display()
        )));
        let _ = std::fs::remove_dir(dir.join(".venv"));

        std::fs::write(dir.join("uv.lock"), "").unwrap();
        assert_eq!(coverage_fallback_command(&dir, &cov_file, &sources), venv);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn coverage_fallback_reports_a_failed_python3_run() {
        let dir = scratch("cov-run");
        let cov_file = dir.join(".sc/coverage/pytest.json");
        let mut runs = Vec::new();
        let err = run_coverage_fallback(
            &dir,
            Instant::now() + Duration::from_secs(20),
            &["src".into()],
            &mut runs,
        )
        .expect_err("an empty tree does not produce coverage");
        assert!(err.starts_with("coverage.py failed"), "{err}");
        assert_ne!(err, "coverage.py failed", "{err}");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].engine, "coverage");
        assert_eq!(
            runs[0].command,
            coverage_fallback_command(&dir, &cov_file, &["src".into()])
        );
        assert!(runs[0]
            .command
            .starts_with("PYTHONPATH='src'${PYTHONPATH:+:\"$PYTHONPATH\"} python3 -m coverage run --source='src' "));
        assert_ne!(runs[0].exit_code, Some(0));
        assert!(dir.join(".sc/coverage").is_dir());
        let _ = std::fs::remove_dir_all(&dir);
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

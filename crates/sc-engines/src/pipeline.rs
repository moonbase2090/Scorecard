// SPDX-License-Identifier: MPL-2.0
//! Assemble a scorecard from the deterministic engines.
//!
//! Engines return structured findings. Nothing in this crate writes user-facing
//! diagnostics; the CLI formats the scorecard.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use sc_core::{
    apply_disposition, compute_scores, CrapSection, Finding, Gate, GitInfo, LlmSection, Metrics,
    MutationSection, RunRecord, Scope, Scorecard, SpecSection, SCORECARD_VERSION,
};
use sc_graph::FunctionInfo;

use crate::cargo_test::{targeted_test_names, test_findings};
use crate::command::{run_cargo, run_cmd, CommandError};
use crate::compile::{generic_compile_failure, parse_compiler_messages};
use crate::coverage::parse_coverage_json;
use crate::crap::{evaluate, unmatched_functions};

use crate::mutation::run_mutation;
use crate::scope::{empty_selection, select, Selection};
use crate::spec_check::{check_spec, gap_value};

#[derive(Debug, Clone)]
pub struct AnalyzeRequest {
    pub root: PathBuf,
    pub repo: String,
    pub fail_on: Vec<String>,
    pub budget: Duration,
    pub config: sc_core::Config,
    pub diff_base: Option<String>,
    pub diff_head: Option<String>,
    pub path_list: Vec<String>,
    pub spec_path: Option<PathBuf>,
    pub mutation_override: Option<String>,
    pub llm_override: Option<bool>,
    pub intent: Option<String>,
}

const MAX_UNMATCHED_FUNCTION_EVIDENCE: usize = 20;

#[derive(serde::Serialize)]
struct UnmatchedFunctionEvidence<'a> {
    file: &'a str,
    symbol: &'a str,
    line: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    Passed,
    GateFailed,
    AnalyzerError,
}

#[derive(Debug)]
pub struct AnalyzeOutput {
    pub scorecard: Scorecard,
    pub status: RunStatus,
}

pub fn analyze(request: AnalyzeRequest) -> AnalyzeOutput {
    // Snapshot git status before any engine runs. Engines create files under
    // the root (target/, coverage artifacts, .sc/last-scorecard.json), so a
    // probe at the end of analysis flags sc's own outputs as a dirty tree.
    let git = git_info(&request.root);
    match crate::pack::detect(&request.root, &request.config.pack) {
        Ok(crate::pack::Detected::Pack(crate::pack::PackId::Rust)) => analyze_rust(request, git),
        Ok(crate::pack::Detected::Pack(crate::pack::PackId::Python)) => {
            analyze_python(request, git)
        }
        Ok(crate::pack::Detected::Pack(crate::pack::PackId::Web)) => analyze_web(request, git),
        Ok(crate::pack::Detected::Pack(pack)) => analyze_unsupported(request, pack, git),
        Ok(crate::pack::Detected::Unknown) => {
            analyze_blocked(request, "unknown", "no language pack detected", git)
        }
        Ok(crate::pack::Detected::Ambiguous(packs)) => {
            let names: Vec<_> = packs.iter().map(|pack| pack.as_str()).collect();
            analyze_blocked(
                request,
                "ambiguous",
                &format!(
                    "several language packs match ({}); set pack in analyzer.toml or pass --pack",
                    names.join(", ")
                ),
                git,
            )
        }
        Err(err) => analyze_blocked(request, "unknown", &err, git),
    }
}

fn analyze_blocked(
    request: AnalyzeRequest,
    pack: &str,
    message: &str,
    git: GitInfo,
) -> AnalyzeOutput {
    let findings = vec![
        unavailable("compile", message),
        unavailable("tests", message),
        unavailable("crap", message),
        unavailable("sca", message),
        unavailable("lint", message),
    ];
    let gates = vec![
        gate("types", false, message),
        gate("tests", false, message),
        gate("crap", false, message),
        Gate {
            id: "sca".into(),
            pass: false,
            reason: Some(message.to_string()),
            enforced: false,
        },
        gate("lint", false, message),
    ];
    finish(Draft {
        root: request.root.clone(),
        repo: request.repo,
        pack: pack.to_string(),
        test_selection: "full-suite".into(),
        git,
        mode: "tree".into(),
        paths: Vec::new(),
        base: None,
        fail_on: request.fail_on,
        findings,
        ran: Vec::new(),
        skipped: vec![
            "compile".into(),
            "tests".into(),
            "coverage".into(),
            "complexity".into(),
            "crap".into(),
            "sca".into(),
            "lint".into(),
            "llm".into(),
            "mutation".into(),
        ],
        gates,
        metrics: sc_core::Metrics::zeros(),
        threshold: request.config.gates.crap_threshold,
        worst: Vec::new(),
        mutation: MutationSection::skipped(),
        spec: SpecSection::empty(),
        intent: request.intent.clone(),
        llm: other_pack_llm(request.llm_override.unwrap_or(request.config.llm.enabled)),
        runs: Vec::new(),
        analyzer_error: true,
    })
}

fn analyze_web(request: AnalyzeRequest, git: GitInfo) -> AnalyzeOutput {
    let mut findings = Vec::new();
    let mut functions = crate::poly_cc::functions_for_pack(&request.root, "node");
    for rel in html_files(&request.root) {
        let Ok(text) = std::fs::read_to_string(request.root.join(&rel)) else {
            continue;
        };
        let (html, parsed) = crate::html_doc::html_findings(&rel, &text);
        findings.extend(html);
        findings.extend(crate::links::link_findings(
            &request.root,
            &rel,
            &parsed.elements,
        ));
        findings.extend(crate::a11y::check_elements(
            &rel,
            &parsed.elements,
            &request.config.a11y.disable,
            true,
        ));
        for script in parsed.scripts {
            functions.extend(crate::poly_cc::javascript_in(
                &rel,
                &script.body,
                script.line.saturating_sub(1),
            ));
        }
    }
    let secrets = crate::pack::text_secrets(&request.root);
    let secret_errors = secrets
        .iter()
        .filter(|finding| finding.severity == "error")
        .count();
    findings.extend(secrets);
    let html_errors = findings
        .iter()
        .filter(|finding| finding.engine == "html" && finding.severity == "error")
        .count();
    let missing_links = findings
        .iter()
        .filter(|finding| finding.rule == "links.missing")
        .count();
    let crap = crate::crap::evaluate(
        &functions,
        None,
        request.config.gates.crap_threshold,
        request.config.gates.new_fn_untested_cc,
        |_| true,
    );
    if !crap.coverage_complete {
        findings.push(crate::coverage::missing_finding(
            "coverage report is not collected for this pack",
        ));
    }
    findings.extend(crap.findings.clone());
    let html_enforced = html_gate_enforced(&request.config, &request.fail_on);
    let links_enforced =
        request.config.links.enforce || request.fail_on.iter().any(|gate| gate == "links");
    let a11y_enforced =
        request.config.a11y.enforce || request.fail_on.iter().any(|gate| gate == "a11y");
    let a11y_count = findings
        .iter()
        .filter(|finding| finding.engine == "a11y")
        .count();
    let mut fail_on = request.fail_on.clone();
    if html_enforced && !fail_on.iter().any(|gate| gate == "html") {
        fail_on.push("html".into());
    }
    if links_enforced && !fail_on.iter().any(|gate| gate == "links") {
        fail_on.push("links".into());
    }
    if a11y_enforced && !fail_on.iter().any(|gate| gate == "a11y") {
        fail_on.push("a11y".into());
    }
    let gates = vec![
        mode_gate(
            "html",
            html_errors == 0,
            html_enforced,
            &if html_errors == 0 {
                String::new()
            } else {
                format!("{html_errors} markup findings")
            },
        ),
        mode_gate(
            "a11y",
            a11y_count == 0,
            a11y_enforced,
            &if a11y_count == 0 {
                String::new()
            } else {
                format!("{a11y_count} accessibility findings")
            },
        ),
        mode_gate(
            "links",
            missing_links == 0,
            links_enforced,
            &if missing_links == 0 {
                String::new()
            } else {
                format!("{missing_links} missing files")
            },
        ),
        crap_gate(
            crap.coverage_complete,
            crap.over,
            crap.untested,
            request.config.gates.new_fn_untested_cc,
        ),
        gate(
            "secrets",
            secret_errors == 0,
            &if secret_errors == 0 {
                String::new()
            } else {
                format!("{secret_errors} secrets")
            },
        ),
    ];
    finish(Draft {
        root: request.root.clone(),
        repo: request.repo,
        pack: "web".into(),
        test_selection: "full-suite".into(),
        git,
        mode: "tree".into(),
        paths: Vec::new(),
        base: None,
        fail_on,
        findings,
        ran: {
            let mut ran = vec![
                "html".into(),
                "a11y".into(),
                "links".into(),
                "secrets".into(),
                "crap".into(),
            ];
            if !functions.is_empty() {
                ran.push("complexity".into());
            }
            ran
        },
        skipped: {
            let mut skipped = vec![
                "compile".into(),
                "tests".into(),
                "coverage".into(),
                "sca".into(),
                "lint".into(),
                "llm".into(),
                "mutation".into(),
            ];
            if functions.is_empty() {
                skipped.push("complexity".into());
            }
            skipped
        },
        gates,
        metrics: sc_core::Metrics {
            loc_changed: 0,
            files_changed: html_files(&request.root).len() as u64,
            coverage_changed: 0.0,
            crap_max: crap.crap_max,
            crap_over_threshold: crap.over,
            hallucinated_imports: 0,
            undeclared_dependencies: 0,
        },
        threshold: request.config.gates.crap_threshold,
        worst: crap.worst,
        mutation: MutationSection::skipped(),
        spec: SpecSection::empty(),
        intent: request.intent.clone(),
        llm: other_pack_llm(request.llm_override.unwrap_or(request.config.llm.enabled)),
        runs: Vec::new(),
        analyzer_error: false,
    })
}

fn markup_files(root: &Path, exts: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(root: &Path, dir: &Path, depth: u32, exts: &[&str], out: &mut Vec<String>) {
        if depth > 6 || out.len() >= 200 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if name.starts_with('.') || name == "node_modules" || name == "dist" || name == "target"
            {
                continue;
            }
            if path.is_dir() {
                walk(root, &path, depth + 1, exts, out);
            } else if path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| exts.contains(&ext))
            {
                if let Ok(rel) = path.strip_prefix(root) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }
    walk(root, root, 0, exts, &mut out);
    out.sort();
    out
}

fn html_files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(root: &Path, dir: &Path, depth: u32, out: &mut Vec<String>) {
        if depth > 6 || out.len() >= 200 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if name.starts_with('.') || name == "node_modules" || name == "dist" || name == "target"
            {
                continue;
            }
            if path.is_dir() {
                walk(root, &path, depth + 1, out);
            } else if matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("html" | "htm")
            ) {
                if let Ok(rel) = path.strip_prefix(root) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }
    walk(root, root, 0, &mut out);
    out.sort();
    out
}

fn html_gate_enforced(config: &sc_core::Config, fail_on: &[String]) -> bool {
    match config.html.enforce.trim().to_ascii_lowercase().as_str() {
        "on" | "true" | "enforced" => true,
        "off" | "false" | "advisory" => false,
        _ => {
            fail_on.iter().any(|gate| gate == "html") || {
                let builtin = ["types", "tests", "crap", "secrets", "lint"];
                fail_on.len() == builtin.len()
                    && builtin
                        .iter()
                        .all(|gate| fail_on.iter().any(|item| item == gate))
            }
        }
    }
}

fn mode_gate(id: &str, pass: bool, enforced: bool, reason: &str) -> Gate {
    let reason = if pass {
        None
    } else if enforced || reason.is_empty() {
        Some(reason.to_string())
    } else {
        Some(format!("{reason} (advisory; does not fail the process)"))
    };
    Gate {
        id: id.to_string(),
        pass,
        enforced,
        reason,
    }
}

fn analyze_unsupported(
    request: AnalyzeRequest,
    pack: crate::pack::PackId,
    git: GitInfo,
) -> AnalyzeOutput {
    let lint_is_rust_default = request.config.commands.lint.trim()
        == crate::pack::PackId::Rust.lint_default()
        || request.config.commands.lint.trim().is_empty();
    let user_lint = if lint_is_rust_default {
        None
    } else {
        Some(request.config.commands.lint.trim())
    };
    let tools = crate::toolchain::run(
        pack,
        &request.root,
        Instant::now() + request.budget,
        user_lint,
    );
    let mut findings = tools.findings;
    findings.push(unavailable(
        "sca",
        "dependency check is not implemented for this pack",
    ));
    let secrets = crate::pack::text_secrets(&request.root);
    let secret_errors = secrets
        .iter()
        .filter(|finding| finding.severity == "error")
        .count();
    findings.extend(secrets);
    let jsx_files = if pack == crate::pack::PackId::Node {
        markup_files(&request.root, &["jsx", "tsx"])
    } else {
        Vec::new()
    };
    for rel in &jsx_files {
        let Ok(text) = std::fs::read_to_string(request.root.join(rel)) else {
            continue;
        };
        findings.extend(crate::a11y::check_jsx(
            rel,
            &text,
            &request.config.a11y.disable,
        ));
    }
    let a11y_count = findings
        .iter()
        .filter(|finding| finding.engine == "a11y")
        .count();
    let mut fail_on = request.fail_on.clone();
    let mut gates = tools.gates;
    if !jsx_files.is_empty() {
        let enforced = request.config.a11y.enforce || fail_on.iter().any(|gate| gate == "a11y");
        if enforced && !fail_on.iter().any(|gate| gate == "a11y") {
            fail_on.push("a11y".into());
        }
        gates.push(mode_gate(
            "a11y",
            a11y_count == 0,
            enforced,
            &if a11y_count == 0 {
                String::new()
            } else {
                format!("{a11y_count} accessibility findings")
            },
        ));
    }
    gates.push(gate_reported("sca"));
    let mut ran = tools.ran;
    let mut skipped = tools.skipped;
    let runs = tools.runs;
    let functions = crate::poly_cc::functions_for_pack(&request.root, pack.as_str());
    let coverage = if pack == crate::pack::PackId::Go {
        std::fs::read_to_string(crate::toolchain::go_cover_path(&request.root))
            .ok()
            .map(|text| crate::poly_cc::go_coverage(&text, &functions))
    } else {
        crate::pack_cov::load(pack.as_str(), &request.root, &functions)
    };
    let crap = crate::crap::evaluate(
        &functions,
        coverage.as_ref(),
        request.config.gates.crap_threshold,
        request.config.gates.new_fn_untested_cc,
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
    }
    findings.extend(crap.findings.clone());
    gates.push(crap_gate(
        crap.coverage_complete,
        crap.over,
        crap.untested,
        request.config.gates.new_fn_untested_cc,
    ));
    gates.push(gate(
        "secrets",
        secret_errors == 0,
        &if secret_errors == 0 {
            String::new()
        } else {
            format!("{secret_errors} secrets")
        },
    ));
    finish(Draft {
        root: request.root.clone(),
        repo: request.repo,
        pack: pack.as_str().to_string(),
        test_selection: pack.test_selection().into(),
        git,
        mode: "tree".into(),
        paths: Vec::new(),
        base: None,
        fail_on,
        findings,
        ran: {
            ran.push("secrets".into());
            ran.push("crap".into());
            if coverage.is_some() {
                ran.push("coverage".into());
            }
            ran
        },
        skipped: {
            if coverage.is_none() {
                skipped.push("coverage".into());
            }
            skipped.push("complexity".into());
            skipped.push("llm".into());
            skipped.push("mutation".into());
            skipped
        },
        gates,
        metrics: sc_core::Metrics {
            loc_changed: 0,
            files_changed: 0,
            coverage_changed: 0.0,
            crap_max: crap.crap_max,
            crap_over_threshold: crap.over,
            hallucinated_imports: 0,
            undeclared_dependencies: 0,
        },
        threshold: request.config.gates.crap_threshold,
        worst: crap.worst,
        mutation: MutationSection::skipped(),
        spec: SpecSection::empty(),
        intent: request.intent.clone(),
        llm: other_pack_llm(request.llm_override.unwrap_or(request.config.llm.enabled)),
        runs,
        analyzer_error: false,
    })
}

fn analyze_python(request: AnalyzeRequest, git: GitInfo) -> AnalyzeOutput {
    let deadline = Instant::now() + request.budget;
    let outcome = crate::python::run(
        &request.root,
        deadline,
        request.config.gates.crap_threshold,
        request.config.gates.new_fn_untested_cc,
    );
    let gates = vec![
        if outcome.types_pass {
            gate("types", true, "")
        } else {
            gate("types", false, &outcome.types_reason)
        },
        if outcome.tests_enforced {
            gate("tests", outcome.tests_pass, &outcome.tests_reason)
        } else {
            gate_reported("tests")
        },
        crap_gate(
            outcome.crap_coverage_complete,
            outcome.crap_over,
            outcome.crap_untested,
            request.config.gates.new_fn_untested_cc,
        ),
        advisory_detail(
            "sca",
            outcome.sca.total(),
            &sca_detail(
                outcome.sca.undeclared,
                outcome.sca.hallucinated,
                outcome.sca.unresolved,
            ),
        ),
        gate("lint", outcome.lint_pass, &outcome.lint_reason),
        gate(
            "secrets",
            outcome.secret_errors == 0,
            &if outcome.secret_errors == 0 {
                String::new()
            } else {
                format!("{} secrets", outcome.secret_errors)
            },
        ),
    ];
    finish(Draft {
        root: request.root.clone(),
        repo: request.repo,
        pack: "python".into(),
        test_selection: "full-suite".into(),
        git,
        mode: "tree".into(),
        paths: Vec::new(),
        base: None,
        fail_on: request.fail_on,
        findings: outcome.findings,
        ran: outcome.ran,
        skipped: outcome.skipped,
        gates,
        metrics: sc_core::Metrics {
            loc_changed: 0,
            files_changed: 0,
            coverage_changed: 0.0,
            crap_max: outcome.crap_max,
            crap_over_threshold: outcome.crap_over,
            hallucinated_imports: outcome.sca.hallucinated,
            undeclared_dependencies: outcome.sca.undeclared,
        },
        threshold: request.config.gates.crap_threshold,
        worst: outcome.crap_worst,
        mutation: MutationSection::skipped(),
        spec: SpecSection::empty(),
        intent: request.intent.clone(),
        llm: other_pack_llm(request.llm_override.unwrap_or(request.config.llm.enabled)),
        runs: outcome.runs,
        analyzer_error: false,
    })
}

fn analyze_rust(request: AnalyzeRequest, git: GitInfo) -> AnalyzeOutput {
    let deadline = Instant::now() + request.budget;
    let root = &request.root;
    let threshold = request.config.gates.crap_threshold;
    let (selection, select_error) = match select(
        root,
        &request.config.scope.exclude,
        request.diff_base.as_deref(),
        request.diff_head.as_deref(),
        &request.path_list,
    ) {
        Ok(selection) => (selection, None),
        Err(err) => (empty_selection(), Some(err)),
    };

    let mut state = RustState::new();
    if let Some(err) = &select_error {
        state.findings.push(unavailable("scope", err));
    }
    state.analyzer_error = select_error.is_some();

    let manifest = root.join("Cargo.toml");
    if !manifest.is_file() {
        missing_manifest(&mut state);
    } else {
        compile_phase(root, &manifest, deadline, &mut state);

        test_phase(root, &selection, &manifest, deadline, &mut state);
        coverage_phase(
            root,
            &selection,
            &manifest,
            deadline,
            request.config.engines.coverage,
            &mut state,
        );
    }

    assemble_rust_report(&request, &selection, threshold, deadline, state, git)
}

struct RustState {
    findings: Vec<Finding>,
    runs: Vec<RunRecord>,
    ran: Vec<String>,
    skipped: Vec<String>,
    analyzer_error: bool,
    types_pass: bool,
    types_reason: String,
    tests_pass: bool,
    tests_reason: String,
    coverage_data: Option<crate::coverage::CoverageData>,
    line_rate: f64,
}

impl RustState {
    fn new() -> Self {
        Self {
            findings: Vec::new(),
            runs: Vec::new(),
            ran: vec!["complexity".to_string()],
            skipped: Vec::new(),
            analyzer_error: false,
            types_pass: false,
            types_reason: String::new(),
            tests_pass: false,
            tests_reason: String::new(),
            coverage_data: None,
            line_rate: 0.0,
        }
    }
}

fn missing_manifest(state: &mut RustState) {
    state.analyzer_error = true;
    state.types_pass = false;
    state.types_reason = "no Cargo.toml found".into();
    state.tests_pass = false;
    state.tests_reason = "no Cargo.toml found".into();
    state.findings.push(unavailable(
        "compile",
        "no Cargo.toml found; cargo check was not run",
    ));
    state.findings.push(unavailable(
        "tests",
        "no Cargo.toml found; cargo test was not run",
    ));
    state
        .skipped
        .extend(["compile".into(), "tests".into(), "coverage".into()]);
}

fn compile_phase(root: &Path, manifest: &Path, deadline: Instant, state: &mut RustState) {
    match run_check(root, manifest, deadline) {
        Ok(captured) => {
            note_run(
                &mut state.runs,
                "compile",
                "cargo check --message-format=json",
                captured.status.code(),
                captured.elapsed,
            );
            state.ran.push("compile".into());
            let mut errors = parse_compiler_messages(root, &captured.stdout);
            if !captured.status.success() && errors.is_empty() {
                errors.push(generic_compile_failure(&captured.stdout, &captured.stderr));
            }
            state.types_pass = errors.is_empty();
            if !state.types_pass {
                state.types_reason = "compiler errors".into();
            }
            state.findings.extend(errors);
        }
        Err(CommandError::Timeout) => {
            state.analyzer_error = true;
            state.types_pass = false;
            state.types_reason = "timed out".into();
            note_run(
                &mut state.runs,
                "compile",
                "cargo check --message-format=json",
                None,
                Duration::ZERO,
            );
            state
                .findings
                .push(unavailable("compile", "cargo check timed out"));
            state.skipped.push("compile".into());
        }
        Err(err) => {
            state.analyzer_error = true;
            state.types_pass = false;
            state.types_reason = "toolchain unavailable".into();
            state
                .findings
                .push(unavailable("compile", &err.message("cargo check")));
            state.skipped.push("compile".into());
        }
    }
}

fn test_phase(
    root: &Path,
    selection: &Selection,
    manifest: &Path,
    deadline: Instant,
    state: &mut RustState,
) {
    if !state.types_pass {
        state.tests_pass = false;
        state.tests_reason = if state.analyzer_error {
            state.types_reason.clone()
        } else {
            "blocked by compile errors".into()
        };
        if state.skipped.iter().any(|engine| engine == "compile") {
            state.findings.push(unavailable(
                "tests",
                "cargo test was not run because cargo check did not start",
            ));
        }
        state.skipped.extend(["tests".into(), "coverage".into()]);
        return;
    }
    let symbols: Vec<String> = selection
        .crap_functions
        .iter()
        .map(|function| function.symbol.clone())
        .collect();
    let targeted = if selection.mode == "diff" {
        targeted_test_names(root, &symbols)
    } else {
        Vec::new()
    };
    match run_test_set(root, manifest, deadline, &targeted, &mut state.runs) {
        Ok(captured) => {
            state.ran.push("tests".into());
            let failures =
                test_findings(root, &captured.stdout, &captured.stderr, captured.success);
            state.tests_pass = failures.is_empty();
            if !state.tests_pass {
                state.tests_reason = "test failures".into();
            }
            state.findings.extend(failures);
        }
        Err(CommandError::Timeout) => {
            state.analyzer_error = true;
            state.tests_pass = false;
            state.tests_reason = "timed out".into();
            state
                .findings
                .push(unavailable("tests", "cargo test timed out"));
            state.skipped.push("tests".into());
        }
        Err(err) => {
            state.analyzer_error = true;
            state.tests_pass = false;
            state.tests_reason = "toolchain unavailable".into();
            state
                .findings
                .push(unavailable("tests", &err.message("cargo test")));
            state.skipped.push("tests".into());
        }
    }
}

fn coverage_phase(
    root: &Path,
    selection: &Selection,
    manifest: &Path,
    deadline: Instant,
    coverage_enabled: bool,
    state: &mut RustState,
) {
    if !state.tests_pass {
        if !state.skipped.iter().any(|engine| engine == "coverage") {
            state.skipped.push("coverage".into());
        }
        let failed_test = state
            .runs
            .iter()
            .rev()
            .find(|run| run.engine == "tests" && run.exit_code != Some(0));
        let reason = match failed_test {
            Some(run) => format!(
                "coverage skipped: tests gate failed (test command exit code {})",
                run.exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "unavailable".into())
            ),
            None => format!(
                "coverage skipped: tests gate failed ({})",
                if state.tests_reason.is_empty() {
                    "test command was not run"
                } else {
                    &state.tests_reason
                }
            ),
        };
        state
            .findings
            .push(crate::coverage::missing_finding(&reason));
        return;
    }
    if !coverage_enabled {
        if !state.skipped.iter().any(|engine| engine == "coverage") {
            state.skipped.push("coverage".into());
        }
        return;
    }
    match run_coverage(root, manifest, deadline, &mut state.runs) {
        Ok(data) => {
            state.ran.push("coverage".into());
            let unmatched: Vec<_> = unmatched_functions(&selection.crap_functions, &data).collect();
            if !unmatched.is_empty() {
                let mut finding = Finding {
                    id: "coverage:unmatched".into(),
                    rule: "coverage.unmatched".into(),
                    engine: "coverage".into(),
                    severity: "warning".into(),
                    file: ".".into(),
                    span: None,
                    symbol: None,
                    message: format!(
                        "coverage was not measured for {} analyzed function(s) without llvm-cov records",
                        unmatched.len()
                    ),
                    evidence: serde_json::json!({
                        "unmatched": unmatched.len(),
                        "functions": unmatched
                            .iter()
                            .take(MAX_UNMATCHED_FUNCTION_EVIDENCE)
                            .map(|function| UnmatchedFunctionEvidence {
                                file: &function.file,
                                symbol: &function.symbol,
                                line: function.span.start_line,
                            })
                            .collect::<Vec<_>>(),
                    }),
                    suggested_action: Some(
                        "Check that the function is compiled into the test binary".into(),
                    ),
                    disposition: String::new(),
                };
                if let [function] = unmatched.as_slice() {
                    finding.file = function.file.clone();
                    finding.symbol = Some(function.symbol.clone());
                    finding.span = Some(function.span.clone());
                }
                state.findings.push(finding);
            }
            state.line_rate = data.line_rate;
            state.coverage_data = Some(data);
        }
        Err(err) => {
            state.skipped.push("coverage".into());
            state
                .findings
                .push(crate::coverage::missing_finding(&format!(
                    "coverage tooling unavailable: {err}"
                )));
        }
    }
}

fn lint_gate(findings: &[Finding], fail_on: &[String]) -> Option<Gate> {
    let lint_failed = findings.iter().any(|finding| finding.rule == "lint.failed");
    let lint_missing = findings
        .iter()
        .any(|finding| finding.engine == "lint" && finding.rule == "engine.unavailable");
    let required = fail_on.iter().any(|gate| gate == "lint");
    let pass = !lint_failed && !(lint_missing && required);
    let reason = if lint_failed {
        "lint failed"
    } else if lint_missing && required {
        "lint unavailable"
    } else {
        ""
    };
    Some(gate("lint", pass, reason))
}

fn push_optional_gates(
    gates: &mut Vec<Gate>,
    request: &AnalyzeRequest,
    state: &RustState,
    spec: &SpecRun,
    mutation: &MutationRun,
    undeclared: u64,
    report_lint: bool,
) {
    if request.config.engines.sca {
        gates.push(advisory_gate("sca", undeclared, "undeclared dependencies"));
    }
    let secret_errors = state
        .findings
        .iter()
        .filter(|finding| finding.engine == "secrets" && finding.severity == "error")
        .count();
    gates.push(gate(
        "secrets",
        secret_errors == 0,
        &if secret_errors == 0 {
            String::new()
        } else {
            format!("{secret_errors} secrets")
        },
    ));
    if spec.ran_gate {
        gates.push(gate("spec", spec.pass, &spec.reason));
    }
    if mutation.add_gate {
        gates.push(gate("mutation", mutation.pass, &mutation.reason));
    }
    if report_lint {
        if let Some(lint) = lint_gate(&state.findings, &request.fail_on) {
            gates.push(lint);
        }
    }
}

fn assemble_rust_report(
    request: &AnalyzeRequest,
    selection: &Selection,
    threshold: u32,
    deadline: Instant,
    mut state: RustState,
    git: GitInfo,
) -> AnalyzeOutput {
    let root = &request.root;
    let report_lint = run_lint_engine(
        root,
        &request.config.commands.lint,
        state.types_pass,
        deadline,
        LintSink {
            ran: &mut state.ran,
            skipped: &mut state.skipped,
            findings: &mut state.findings,
            runs: &mut state.runs,
        },
    );

    let untested_cc = request.config.gates.new_fn_untested_cc;
    let new_symbols = selection.new_symbols.clone();
    let narrow_untested = selection.narrow_untested;
    let crap = evaluate(
        &selection.crap_functions,
        state.coverage_data.as_ref(),
        threshold,
        untested_cc,
        |function| {
            !narrow_untested
                || new_symbols.contains(&(function.file.clone(), function.symbol.clone()))
        },
    );
    state.ran.push("crap".into());
    if !crap.coverage_complete
        && !state
            .findings
            .iter()
            .any(|finding| finding.engine == "coverage")
    {
        state.findings.push(crate::coverage::missing_finding(
            "coverage was not run for all analyzed functions",
        ));
    }
    let crap_gate = crap_gate(
        crap.coverage_complete,
        crap.over,
        crap.untested,
        untested_cc,
    );
    state.findings.extend(crap.findings);

    let undeclared = import_findings(
        root,
        selection,
        request.config.engines.sca,
        &mut state.ran,
        &mut state.skipped,
        &mut state.findings,
    );
    secret_and_perf(selection, &mut state.ran, &mut state.findings);
    let mut spec = spec_engine(
        request,
        selection,
        &mut state.ran,
        &mut state.skipped,
        &mut state.findings,
    );
    let mutation = mutation_engine(
        request,
        &mut state.ran,
        &mut state.skipped,
        &mut state.findings,
    );
    let llm = llm_engine(
        request,
        &mut spec,
        &mut state.ran,
        &mut state.skipped,
        &mut state.findings,
        &selection.paths,
    );

    let coverage_changed = if selection.mode == "tree" {
        state.line_rate
    } else {
        mean_coverage(&selection.crap_functions, state.coverage_data.as_ref())
    };
    let metrics = Metrics {
        loc_changed: selection.loc_changed,
        files_changed: selection.files_changed,
        coverage_changed,
        crap_max: crap.crap_max,
        crap_over_threshold: crap.over,
        hallucinated_imports: 0,
        undeclared_dependencies: undeclared,
    };

    let mut gates = vec![
        gate("types", state.types_pass, &state.types_reason),
        gate("tests", state.tests_pass, &state.tests_reason),
        crap_gate,
    ];
    push_optional_gates(
        &mut gates,
        request,
        &state,
        &spec,
        &mutation,
        undeclared,
        report_lint,
    );

    finish(Draft {
        root: root.to_path_buf(),
        repo: request.repo.clone(),
        pack: "rust".into(),
        test_selection: if request.diff_base.is_some() {
            "rust-tests".into()
        } else {
            "full-suite".into()
        },
        git,
        mode: selection.mode.clone(),
        paths: selection.paths.clone(),
        base: selection.base.clone(),
        fail_on: request.fail_on.clone(),
        findings: state.findings,
        ran: state.ran,
        skipped: state.skipped,
        gates,
        metrics,
        threshold,
        worst: crap.worst,
        mutation: mutation.section,
        spec: spec.section,
        intent: request.intent.clone(),
        llm,
        runs: state.runs,
        analyzer_error: state.analyzer_error,
    })
}

struct Draft {
    root: PathBuf,
    repo: String,
    pack: String,
    test_selection: String,
    git: GitInfo,
    mode: String,
    paths: Vec<String>,
    base: Option<String>,
    fail_on: Vec<String>,
    findings: Vec<Finding>,
    ran: Vec<String>,
    skipped: Vec<String>,
    gates: Vec<Gate>,
    metrics: Metrics,
    threshold: u32,
    worst: Vec<sc_core::CrapFunction>,
    mutation: MutationSection,
    spec: SpecSection,
    intent: Option<String>,
    llm: Option<LlmSection>,
    runs: Vec<RunRecord>,
    analyzer_error: bool,
}

fn finish(mut draft: Draft) -> AnalyzeOutput {
    order_engines(&mut draft.ran, &mut draft.skipped);
    sort_findings(&mut draft.findings);
    unique_ids(&mut draft.findings);
    apply_disposition(&mut draft.findings);
    sc_core::apply_fail_on(&mut draft.gates, &draft.fail_on);
    let failed = sc_core::verdict_fails(&draft.gates, &draft.fail_on);
    let scores = compute_scores(&draft.findings);
    let scorecard = Scorecard {
        version: SCORECARD_VERSION.to_string(),
        id: sc_core::new_scorecard_id(),
        repo: draft.repo,
        pack: draft.pack,
        test_selection: draft.test_selection,
        git: draft.git,
        scope: Scope {
            mode: draft.mode,
            paths: draft.paths,
            base: draft.base,
        },
        intent: draft.intent,
        verdict: if failed { "fail" } else { "pass" }.to_string(),
        engines_run: draft.ran,
        engines_skipped: draft.skipped,
        scores,
        gates: draft.gates,
        metrics: draft.metrics,
        crap: CrapSection {
            threshold: draft.threshold,
            worst: draft.worst,
        },
        mutation: draft.mutation,
        findings: draft.findings,
        spec: draft.spec,
        llm: draft.llm,
        runs: draft.runs,
    };
    write_last_scorecard(&draft.root, &scorecard);
    let status = if draft.analyzer_error {
        RunStatus::AnalyzerError
    } else if failed {
        RunStatus::GateFailed
    } else {
        RunStatus::Passed
    };
    AnalyzeOutput { scorecard, status }
}

struct TestCapture {
    stdout: String,
    stderr: String,
    success: bool,
}

fn note_run(
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

fn note_run_with_budget(
    runs: &mut Vec<RunRecord>,
    engine: &str,
    command: &str,
    exit_code: Option<i32>,
    elapsed: Duration,
    budget: Duration,
) {
    note_run(runs, engine, command, exit_code, elapsed);
    if let Some(run) = runs.last_mut() {
        run.budget_ms = Some(u64::try_from(budget.as_millis()).unwrap_or(u64::MAX));
    }
}

fn run_test_set(
    root: &Path,
    manifest: &Path,
    deadline: Instant,
    names: &[String],
    runs: &mut Vec<RunRecord>,
) -> Result<TestCapture, CommandError> {
    if names.is_empty() {
        let started = Instant::now();
        let result = run_tests(root, manifest, deadline);
        match &result {
            Ok(captured) => note_run(
                runs,
                "tests",
                "cargo test",
                captured.status.code(),
                captured.elapsed,
            ),
            Err(_) => note_run(runs, "tests", "cargo test", None, started.elapsed()),
        }
        let captured = result?;
        return Ok(TestCapture {
            success: captured.status.success(),
            stdout: captured.stdout,
            stderr: captured.stderr,
        });
    }
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut success = true;
    for name in names {
        let started = Instant::now();
        let command = format!("cargo test {name}");
        let result = run_one_test(root, manifest, deadline, name);
        match &result {
            Ok(captured) => note_run(
                runs,
                "tests",
                &command,
                captured.status.code(),
                captured.elapsed,
            ),
            Err(_) => note_run(runs, "tests", &command, None, started.elapsed()),
        }
        let captured = result?;
        stdout.push_str(&captured.stdout);
        stderr.push_str(&captured.stderr);
        success &= captured.status.success();
    }
    Ok(TestCapture {
        stdout,
        stderr,
        success,
    })
}

fn run_one_test(
    root: &Path,
    manifest: &Path,
    deadline: Instant,
    name: &str,
) -> Result<crate::command::Captured, CommandError> {
    let manifest = manifest.to_string_lossy().to_string();
    run_cargo(
        root,
        &[
            "test",
            name,
            "--manifest-path",
            &manifest,
            "--color",
            "never",
        ],
        deadline,
    )
}

struct LintSink<'a> {
    ran: &'a mut Vec<String>,
    skipped: &'a mut Vec<String>,
    findings: &'a mut Vec<Finding>,
    runs: &'a mut Vec<RunRecord>,
}

fn run_lint_engine(
    root: &Path,
    lint: &str,
    types_pass: bool,
    deadline: Instant,
    sink: LintSink<'_>,
) -> bool {
    if !types_pass {
        sink.skipped.push("lint".into());
        return false;
    }
    let script = lint.trim();
    if script.is_empty() {
        sink.skipped.push("lint".into());
        return false;
    }
    let started = Instant::now();
    match run_shell(root, script, deadline) {
        Ok(captured) => {
            note_run(
                sink.runs,
                "lint",
                script,
                captured.status.code(),
                captured.elapsed,
            );
            if captured.status.success() {
                sink.ran.push("lint".into());
                true
            } else if command_missing(&captured.stderr, &captured.stdout) {
                sink.skipped.push("lint".into());
                sink.findings
                    .push(unavailable("lint", "lint command was not found"));
                true
            } else {
                sink.ran.push("lint".into());
                let detail =
                    crate::command::brief(&format!("{}\n{}", captured.stdout, captured.stderr));
                sink.findings.push(Finding {
                    id: "lint:failed".into(),
                    rule: "lint.failed".into(),
                    engine: "lint".into(),
                    severity: "error".into(),
                    file: ".".into(),
                    span: None,
                    symbol: None,
                    message: if detail.is_empty() {
                        "lint command failed".into()
                    } else {
                        format!("lint command failed: {detail}")
                    },
                    evidence: serde_json::json!({"command": script}),
                    suggested_action: Some("Fix the lint findings and re-run".into()),
                    disposition: String::new(),
                });
                true
            }
        }
        Err(CommandError::Timeout) => {
            note_run(sink.runs, "lint", script, None, started.elapsed());
            sink.skipped.push("lint".into());
            sink.findings
                .push(unavailable("lint", "lint command timed out"));
            true
        }
        Err(err) => {
            note_run(sink.runs, "lint", script, None, started.elapsed());
            sink.skipped.push("lint".into());
            sink.findings
                .push(unavailable("lint", &err.message("lint")));
            true
        }
    }
}

fn command_missing(stderr: &str, stdout: &str) -> bool {
    let text = format!("{stderr}\n{stdout}").to_ascii_lowercase();
    text.contains("no such command") || text.contains("not found")
}

fn run_shell(
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
        .env("CARGO_TERM_COLOR", "never")
        .env("CARGO_TARGET_DIR", root.join("target"));
    run_cmd(&mut cmd, timeout)
}

fn run_check(
    root: &Path,
    manifest: &Path,
    deadline: Instant,
) -> Result<crate::command::Captured, CommandError> {
    let manifest = manifest.to_string_lossy().to_string();
    run_cargo(
        root,
        &[
            "check",
            "--manifest-path",
            &manifest,
            "--message-format=json",
            "--color",
            "never",
        ],
        deadline,
    )
}

fn run_tests(
    root: &Path,
    manifest: &Path,
    deadline: Instant,
) -> Result<crate::command::Captured, CommandError> {
    let manifest = manifest.to_string_lossy().to_string();
    run_cargo(
        root,
        &["test", "--manifest-path", &manifest, "--color", "never"],
        deadline,
    )
}

fn run_coverage(
    root: &Path,
    manifest: &Path,
    deadline: Instant,
    runs: &mut Vec<RunRecord>,
) -> Result<crate::coverage::CoverageData, String> {
    let out_path = root.join("target").join("sc-coverage.json");
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let manifest_s = manifest.to_string_lossy().to_string();
    let out_s = out_path.to_string_lossy().to_string();
    let started = Instant::now();
    let budget = deadline.saturating_duration_since(started);
    let captured = run_cargo(
        root,
        &[
            "llvm-cov",
            "--json",
            "--output-path",
            &out_s,
            "--manifest-path",
            &manifest_s,
        ],
        deadline,
    );
    match &captured {
        Ok(captured) => note_run_with_budget(
            runs,
            "coverage",
            "cargo llvm-cov --json",
            captured.status.code(),
            captured.elapsed,
            budget,
        ),
        Err(_) => note_run_with_budget(
            runs,
            "coverage",
            "cargo llvm-cov --json",
            None,
            started.elapsed(),
            budget,
        ),
    }
    let captured = captured.map_err(|err| err.message("cargo llvm-cov"))?;
    if !captured.status.success() {
        return Err(failure_detail(&captured.stdout, &captured.stderr));
    }
    let text = std::fs::read_to_string(&out_path).map_err(|err| err.to_string())?;
    parse_coverage_json(&text)
}

fn failure_detail(stdout: &str, stderr: &str) -> String {
    fn pick(text: &str) -> Option<String> {
        let lines: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        let line = lines
            .iter()
            .rev()
            .find(|line| {
                let lower = line.to_ascii_lowercase();
                lower.contains("error") || lower.contains("not found") || lower.contains("llvm")
            })
            .copied()
            .or_else(|| lines.last().copied())?;
        let mut out = line.to_string();
        if out.len() > 240 {
            out.truncate(240);
        }
        Some(out)
    }
    pick(stderr)
        .or_else(|| pick(stdout))
        .unwrap_or_else(|| "cargo llvm-cov failed".to_string())
}

fn git_info(root: &Path) -> GitInfo {
    let head = run_git(root, &["rev-parse", "HEAD"]).ok().and_then(|text| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    });
    let dirty = run_git(root, &["status", "--porcelain"])
        .map(|text| !text.trim().is_empty())
        .unwrap_or(false);
    GitInfo { head, dirty }
}

fn run_git(root: &Path, args: &[&str]) -> Result<String, ()> {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(root)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    match run_cmd(&mut cmd, Duration::from_secs(5)) {
        Ok(captured) if captured.status.success() => Ok(captured.stdout),
        _ => Err(()),
    }
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
        suggested_action: Some("Install the required toolchain and re-run".into()),
        disposition: String::new(),
    }
}

fn crap_gate_reason(over: u64, untested: u64, untested_cc: u32) -> String {
    match (over, untested) {
        (0, 0) => String::new(),
        (1, 0) => "1 function over threshold".to_string(),
        (over, 0) => format!("{over} functions over threshold"),
        (0, 1) => format!("1 function at or above CC {untested_cc} with no coverage"),
        (0, untested) => {
            format!("{untested} functions at or above CC {untested_cc} with no coverage")
        }
        (1, 1) => format!(
            "1 function over threshold; 1 function at or above CC {untested_cc} with no coverage"
        ),
        (over, untested) => format!(
            "{over} functions over threshold; {untested} functions at or above CC {untested_cc} with no coverage"
        ),
    }
}

fn crap_gate(coverage_complete: bool, over: u64, untested: u64, untested_cc: u32) -> Gate {
    if !coverage_complete {
        return Gate {
            id: "crap".into(),
            pass: false,
            enforced: false,
            reason: Some("coverage was not measured for all analyzed functions".into()),
        };
    }
    gate(
        "crap",
        over == 0 && untested == 0,
        &crap_gate_reason(over, untested, untested_cc),
    )
}

fn import_findings(
    root: &Path,
    selection: &Selection,
    sca_enabled: bool,
    ran: &mut Vec<String>,
    skipped: &mut Vec<String>,
    findings: &mut Vec<Finding>,
) -> u64 {
    if !sca_enabled {
        skipped.push("sca".into());
        return 0;
    }
    let allowed = crate::manifest::manifest_crate_names(root);
    // First segments that resolve inside the crate are never external:
    // declared `mod` names, `extern crate` aliases, and file modules
    // (`src/score.rs`, `src/foo/mod.rs`) across the analyzed sources.
    let mut local: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for file in &selection.files {
        local.extend(file.local_names.iter().cloned());
        if let Some(module) = file_module(&file.rel) {
            local.insert(module);
        }
    }
    ran.push("sca".into());
    let mut count = 0u64;
    for file in &selection.files {
        for import in &file.imports {
            let name = crate::manifest::normalize(&import.crate_name);
            if allowed.contains(&name) || local.contains(&name) {
                continue;
            }
            count += 1;
            findings.push(Finding {
                id: format!("sca:{}:{}", import.file, import.crate_name),
                rule: "sca.undeclared_dependency".into(),
                engine: "sca".into(),
                severity: "warning".into(),
                file: import.file.clone(),
                span: Some(sc_core::Span {
                    start_line: import.line,
                    start_col: 1,
                    end_line: import.line,
                    end_col: 1,
                }),
                symbol: Some(import.crate_name.clone()),
                message: format!(
                    "Advisory: crate `{}` is used in source and is not in Cargo.toml",
                    import.crate_name
                ),
                evidence: serde_json::json!({"crate": import.crate_name}),
                suggested_action: Some(format!(
                    "Add `{}` to Cargo.toml or remove the import",
                    import.crate_name
                )),
                disposition: String::new(),
            });
        }
    }
    count
}

/// Module name a source file contributes: `src/score.rs` is module
/// `score`, `src/foo/mod.rs` is module `foo`. Crate roots (`main.rs`,
/// `lib.rs`) and a bare `src/mod.rs` contribute nothing.
fn file_module(rel: &str) -> Option<String> {
    let path = Path::new(rel);
    if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
        return None;
    }
    let stem = path.file_stem().and_then(|stem| stem.to_str())?;
    if stem == "mod" {
        let parent = path.parent()?.file_name().and_then(|name| name.to_str())?;
        if parent == "src" {
            return None;
        }
        return Some(parent.to_string());
    }
    if stem == "main" || stem == "lib" {
        return None;
    }
    Some(stem.to_string())
}

fn secret_and_perf(selection: &Selection, ran: &mut Vec<String>, findings: &mut Vec<Finding>) {
    ran.push("secrets".into());
    ran.push("perf".into());
    for file in &selection.files {
        findings.extend(file.secrets.clone());
        for hit in &file.perf {
            findings.push(Finding {
                id: format!("perf:{}:{}:{}", hit.file, hit.symbol, hit.rule),
                rule: hit.rule.clone(),
                engine: "perf".into(),
                severity: "warning".into(),
                file: hit.file.clone(),
                span: Some(hit.span.clone()),
                symbol: Some(hit.symbol.clone()),
                message: hit.message.clone(),
                evidence: serde_json::json!({}),
                suggested_action: Some("Simplify the loop or clone outside it".into()),
                disposition: String::new(),
            });
        }
    }
}

struct SpecRun {
    section: SpecSection,
    ran_gate: bool,
    pass: bool,
    reason: String,
}

fn spec_engine(
    request: &AnalyzeRequest,
    selection: &Selection,
    ran: &mut Vec<String>,
    skipped: &mut Vec<String>,
    findings: &mut Vec<Finding>,
) -> SpecRun {
    let wants = request.spec_path.is_some() || request.fail_on.iter().any(|gate| gate == "spec");
    if !wants {
        skipped.push("spec".into());
        return SpecRun {
            section: SpecSection::empty(),
            ran_gate: false,
            pass: true,
            reason: String::new(),
        };
    }
    let Some(path) = request.spec_path.as_ref() else {
        findings.push(unavailable(
            "spec",
            "fail_on includes spec, and no --spec file was given",
        ));
        return SpecRun {
            section: SpecSection::empty(),
            ran_gate: true,
            pass: false,
            reason: "no spec file".into(),
        };
    };
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            findings.push(unavailable("spec", &format!("cannot read spec: {err}")));
            return SpecRun {
                section: SpecSection {
                    path: Some(path.display().to_string()),
                    gaps: Vec::new(),
                    llm_rounds: None,
                },
                ran_gate: true,
                pass: false,
                reason: "spec unreadable".into(),
            };
        }
    };
    let items: Vec<_> = selection
        .files
        .iter()
        .flat_map(|file| file.items.clone())
        .collect();
    let (gaps, spec_findings) = check_spec(&request.root, &text, &items);
    ran.push("spec".into());
    findings.extend(spec_findings);
    let pass = gaps.is_empty();
    SpecRun {
        section: SpecSection {
            path: Some(path.display().to_string()),
            gaps: gaps.iter().map(gap_value).collect(),
            llm_rounds: None,
        },
        ran_gate: true,
        pass,
        reason: if pass {
            String::new()
        } else {
            format!("{} spec gaps", gaps.len())
        },
    }
}

struct MutationRun {
    section: MutationSection,
    add_gate: bool,
    pass: bool,
    reason: String,
}

fn mutation_engine(
    request: &AnalyzeRequest,
    ran: &mut Vec<String>,
    skipped: &mut Vec<String>,
    findings: &mut Vec<Finding>,
) -> MutationRun {
    let mode = request
        .mutation_override
        .clone()
        .unwrap_or_else(|| request.config.mutation.mode.clone());
    let normalized = mode.trim().to_ascii_lowercase();
    if normalized.is_empty() || normalized == "off" {
        skipped.push("mutation".into());
        return MutationRun {
            section: MutationSection::skipped(),
            add_gate: false,
            pass: true,
            reason: String::new(),
        };
    }
    let base = request.diff_base.clone().unwrap_or_else(|| "AUTO".into());
    let resolved = crate::scope::resolve_base(&request.root, &base).ok();
    let outcome = run_mutation(
        &request.root,
        &normalized,
        resolved.as_deref(),
        request.config.mutation.max_mutants,
        Duration::from_secs(request.config.mutation.budget_seconds).min(request.budget),
    );
    if outcome.ran {
        ran.push("mutation".into());
    } else {
        skipped.push("mutation".into());
    }
    findings.extend(outcome.findings);
    let pass = outcome.unavailable.is_none() && outcome.section.survived == 0;
    let reason = if let Some(message) = &outcome.unavailable {
        message.clone()
    } else if outcome.section.survived > 0 {
        format!("{} mutants survived", outcome.section.survived)
    } else {
        String::new()
    };
    MutationRun {
        section: outcome.section,
        add_gate: true,
        pass,
        reason,
    }
}

fn explain_llm_failure(reason: &str) -> String {
    let lower = reason.to_ascii_lowercase();
    if lower.contains("connection")
        || lower.contains("timed out")
        || lower.contains("tcp")
        || lower.contains("refused")
    {
        return format!(
            "{reason}. The model endpoint did not answer. Start Ollama, or set endpoint under [llm] in analyzer.toml. The default is http://127.0.0.1:11434/v1."
        );
    }
    if reason.contains("--") || reason.contains("analyzer.toml") || reason.contains("[llm]") {
        return reason.to_string();
    }
    format!("{reason}. Check [llm] in analyzer.toml, or re-run with --llm off.")
}

fn other_pack_llm(enabled: bool) -> Option<LlmSection> {
    if enabled {
        Some(LlmSection::skipped(
            "llm is on, but this pack does not run the model review. It runs on a Rust tree. Re-run there, or leave --llm off.",
        ))
    } else {
        None
    }
}

#[derive(Debug)]
enum LlmStart {
    Off,
    Skip(String),
    Run { spec: String, intent: String },
}

fn llm_start(
    enabled: bool,
    spec_path: Option<&Path>,
    intent: Option<&str>,
    paths: &[String],
) -> LlmStart {
    if !enabled {
        return LlmStart::Off;
    }
    let intent = intent.unwrap_or("").trim();
    if let Some(path) = spec_path {
        return match std::fs::read_to_string(path) {
            Ok(text) => LlmStart::Run {
                spec: text,
                intent: intent.to_string(),
            },
            Err(_) => LlmStart::Skip(
                "llm is on, but the --spec file could not be read. Pass a readable file: --spec PATH."
                    .into(),
            ),
        };
    }
    if intent.is_empty() {
        return LlmStart::Skip(
            "llm is on, but neither --spec nor --intent was given. Re-run with --spec PATH or --intent TEXT."
                .into(),
        );
    }
    let mut text = intent.to_string();
    if !paths.is_empty() {
        text.push_str("\n\nPaths in scope:\n");
        let shown = paths.len().min(200);
        for path in paths.iter().take(shown) {
            text.push_str(path);
            text.push('\n');
        }
        if paths.len() > shown {
            text.push_str(&format!("{} more\n", paths.len() - shown));
        }
    }
    LlmStart::Run {
        spec: String::new(),
        intent: text,
    }
}

fn llm_engine(
    request: &AnalyzeRequest,
    spec: &mut SpecRun,
    ran: &mut Vec<String>,
    skipped: &mut Vec<String>,
    findings: &mut Vec<Finding>,
    paths: &[String],
) -> Option<LlmSection> {
    let enabled = request.llm_override.unwrap_or(request.config.llm.enabled);
    let start = llm_start(
        enabled,
        request.spec_path.as_deref(),
        request.intent.as_deref(),
        paths,
    );
    let (spec_text, prompt_intent) = match start {
        LlmStart::Off => {
            skipped.push("llm".into());
            return None;
        }
        LlmStart::Skip(reason) => {
            skipped.push("llm".into());
            findings.push(unavailable("llm", &reason));
            return Some(LlmSection::skipped(reason));
        }
        LlmStart::Run { spec, intent } => (spec, intent),
    };
    let prompt_intent = if prompt_intent.trim().is_empty() {
        None
    } else {
        Some(prompt_intent.as_str())
    };
    let backend = match request.config.llm.backend.trim() {
        "" | "ollama" => "ollama",
        other => other,
    };
    let llm_request = sc_llm::LlmRequest {
        endpoint: &request.config.llm.endpoint,
        model: &request.config.llm.model,
        api_key: None,
        spec: &spec_text,
        root: &request.root,
        intent: prompt_intent,
        max_tool_rounds: request.config.llm.max_tool_rounds,
    };
    let outcome = match request.config.llm.backend.trim() {
        "" | "ollama" => sc_llm::review(llm_request),
        "cursor" => sc_llm::review_cursor(llm_request),
        "openai-compatible" => match sc_llm::env_api_key(&request.config.llm.api_key_env) {
            Ok(key) => sc_llm::review(sc_llm::LlmRequest {
                endpoint: &request.config.llm.base_url,
                model: &request.config.llm.model,
                api_key: Some(&key),
                spec: &spec_text,
                root: &request.root,
                intent: prompt_intent,
                max_tool_rounds: request.config.llm.max_tool_rounds,
            }),
            Err(message) => sc_llm::LlmOutcome {
                gaps: Vec::new(),
                notes: Vec::new(),
                skipped: Some(message),
                rounds: 0,
                model: String::new(),
            },
        },
        other => sc_llm::LlmOutcome {
            gaps: Vec::new(),
            notes: Vec::new(),
            skipped: Some(format!("unknown llm backend {other}")),
            rounds: 0,
            model: String::new(),
        },
    };
    spec.section.llm_rounds = Some(outcome.rounds);
    if let Some(reason) = outcome.skipped {
        skipped.push("llm".into());
        let reason = explain_llm_failure(&reason);
        findings.push(unavailable("llm", &reason));
        return Some(LlmSection::skipped(reason));
    }
    ran.push("llm".into());
    let gap_count = outcome.gaps.len();
    let model = if outcome.model.is_empty() {
        request.config.llm.model.clone()
    } else {
        outcome.model.clone()
    };
    let notes = outcome.notes.clone();
    let rounds = outcome.rounds;
    for gap in outcome.gaps {
        let message = if gap.detail.is_empty() {
            gap.item.clone()
        } else {
            format!("{}: {}", gap.item, gap.detail)
        };
        spec.section.gaps.push(serde_json::json!({
            "kind": "llm",
            "name": gap.item,
            "message": message.clone(),
        }));
        findings.push(Finding {
            id: format!("spec:llm:{}", gap.item.replace(' ', "_")),
            rule: "spec.llm_gap".into(),
            engine: "llm".into(),
            severity: "warning".into(),
            file: ".".into(),
            span: None,
            symbol: Some(gap.item.clone()),
            message,
            evidence: serde_json::json!({"item": gap.item, "detail": gap.detail}),
            suggested_action: Some("Update the code or the spec".into()),
            disposition: String::new(),
        });
    }
    Some(LlmSection::ran(backend, model, rounds, gap_count, notes))
}

fn mean_coverage(
    functions: &[FunctionInfo],
    coverage: Option<&crate::coverage::CoverageData>,
) -> f64 {
    let Some(coverage) = coverage else {
        return 0.0;
    };
    if functions.is_empty() {
        return coverage.line_rate;
    }
    let measured: Vec<f64> = functions
        .iter()
        .filter_map(|function| coverage.for_function(&function.file, &function.symbol))
        .collect();
    if measured.is_empty() {
        0.0
    } else {
        measured.iter().sum::<f64>() / measured.len() as f64
    }
}

fn write_last_scorecard(root: &Path, card: &Scorecard) {
    let path = root.join(".sc").join("last-scorecard.json");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(card) {
        let _ = std::fs::write(path, text);
    }
}

fn gate(id: &str, pass: bool, reason: &str) -> Gate {
    Gate {
        id: id.to_string(),
        pass,
        reason: if pass || reason.is_empty() {
            None
        } else {
            Some(reason.to_string())
        },
        enforced: true,
    }
}

fn advisory_gate(id: &str, count: u64, noun: &str) -> Gate {
    advisory_detail(id, count, &format!("{count} {noun}"))
}

fn advisory_detail(id: &str, count: u64, detail: &str) -> Gate {
    if count == 0 {
        return Gate {
            id: id.to_string(),
            pass: true,
            reason: None,
            enforced: false,
        };
    }
    Gate {
        id: id.to_string(),
        pass: false,
        enforced: false,
        reason: Some(format!("{detail} (advisory; does not fail the process)")),
    }
}

fn sca_detail(undeclared: u64, hallucinated: u64, unresolved: u64) -> String {
    let mut parts = Vec::new();
    if undeclared > 0 {
        let noun = if undeclared == 1 {
            "dependency"
        } else {
            "dependencies"
        };
        parts.push(format!("{undeclared} undeclared {noun}"));
    }
    if hallucinated > 0 {
        let noun = if hallucinated == 1 {
            "import"
        } else {
            "imports"
        };
        parts.push(format!("{hallucinated} hallucinated {noun}"));
    }
    if unresolved > 0 {
        let noun = if unresolved == 1 { "import" } else { "imports" };
        parts.push(format!(
            "{unresolved} {noun} not classified because the package index was not checked"
        ));
    }
    parts.join(", ")
}

fn gate_reported(id: &str) -> Gate {
    Gate {
        id: id.to_string(),
        pass: false,
        reason: Some("not provided by this pack".into()),
        enforced: false,
    }
}

fn order_engines(ran: &mut Vec<String>, skipped: &mut Vec<String>) {
    for name in ["llm", "mutation"] {
        if !ran.iter().any(|engine| engine == name) && !skipped.iter().any(|engine| engine == name)
        {
            skipped.push(name.to_string());
        }
    }
    const ORDER: &[&str] = &[
        "compile",
        "tests",
        "coverage",
        "complexity",
        "crap",
        "sca",
        "html",
        "a11y",
        "links",
        "secrets",
        "perf",
        "spec",
        "lint",
        "mutation",
        "llm",
    ];
    let rank = |name: &str| ORDER.iter().position(|item| *item == name).unwrap_or(100);
    ran.sort_by_key(|name| rank(name));
    ran.dedup();
    skipped.sort_by_key(|name| rank(name));
    skipped.dedup();
    skipped.retain(|name| !ran.iter().any(|ran_name| ran_name == name));
}

fn sort_findings(findings: &mut [Finding]) {
    findings.sort_by(|a, b| {
        let line_a = a.span.as_ref().map(|span| span.start_line).unwrap_or(0);
        let line_b = b.span.as_ref().map(|span| span.start_line).unwrap_or(0);
        (&a.file, line_a, &a.rule, &a.symbol, &a.id)
            .cmp(&(&b.file, line_b, &b.rule, &b.symbol, &b.id))
    });
}

fn unique_ids(findings: &mut [Finding]) {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for finding in findings.iter_mut() {
        let count = seen.entry(finding.id.clone()).or_insert(0);
        *count += 1;
        if *count > 1 {
            finding.id = format!("{}#{}", finding.id, count);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::Config;

    #[test]
    fn intent_alone_is_enough_to_run_the_llm_review() {
        let start = llm_start(true, None, Some("keep the SHA in its own cell"), &[]);
        match start {
            LlmStart::Run { spec, intent } => {
                assert!(spec.is_empty());
                assert!(intent.contains("SHA"));
            }
            other => panic!("expected a run, got {other:?}"),
        }
        match llm_start(true, None, None, &[]) {
            LlmStart::Skip(reason) => assert!(reason.contains("--intent")),
            other => panic!("expected a skip, got {other:?}"),
        }
        assert!(matches!(
            llm_start(false, None, Some("goal"), &[]),
            LlmStart::Off
        ));
        let paths: Vec<String> = (0..250).map(|index| format!("f{index}.rs")).collect();
        match llm_start(true, None, Some("goal"), &paths) {
            LlmStart::Run { intent, .. } => {
                assert!(intent.contains("f0.rs"));
                assert!(intent.contains("f199.rs"));
                assert!(!intent.contains("f200.rs"));
                assert!(intent.contains("50 more"));
            }
            other => panic!("expected a capped run, got {other:?}"),
        }
    }

    #[test]
    fn missing_manifest_is_an_analyzer_error() {
        let dir = std::env::temp_dir().join(format!("sc-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let output = analyze(AnalyzeRequest {
            root: dir.clone(),
            repo: "empty".into(),
            fail_on: vec!["types".into(), "tests".into(), "crap".into()],
            budget: Duration::from_secs(30),
            config: Config::default(),
            diff_base: None,
            diff_head: None,
            path_list: Vec::new(),
            spec_path: None,
            mutation_override: None,
            llm_override: None,
            intent: None,
        });
        assert_eq!(output.status, RunStatus::AnalyzerError);
        assert_eq!(output.scorecard.verdict, "fail");
        assert!(output
            .scorecard
            .findings
            .iter()
            .any(|finding| finding.rule == "engine.unavailable"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn git_repo(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let run = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success());
        };
        run(&["init", "-q"]);
        run(&[
            "-c",
            "user.email=sc-test@example.com",
            "-c",
            "user.name=sc-test",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "init",
        ]);
        dir
    }

    #[test]
    fn git_probe_sees_clean_tree_and_later_modifications() {
        let dir = git_repo("sc-git-probe");
        let clean = git_info(&dir);
        assert_eq!(clean.head.as_ref().unwrap().len(), 40);
        assert!(!clean.dirty);
        std::fs::write(dir.join("note.txt"), "x").unwrap();
        assert!(git_info(&dir).dirty);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn analyzer_own_outputs_trip_a_late_probe() {
        // Dogfood root cause: files sc itself creates read as a dirty tree
        // once written, so the probe must run before the engines, not after.
        let dir = git_repo("sc-git-own-outputs");
        assert!(!git_info(&dir).dirty);
        std::fs::create_dir_all(dir.join(".sc")).unwrap();
        std::fs::write(dir.join(".sc").join("last-scorecard.json"), "{}").unwrap();
        std::fs::write(dir.join("scorecard.html"), "x").unwrap();
        assert!(
            git_info(&dir).dirty,
            "own outputs look dirty after the fact"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn blocked_analysis_reports_pre_analysis_git_state() {
        // analyze() snapshots git before any engine runs: a clean repo with
        // no detectable pack reports clean with the committed head, and an
        // empty --fail-on set reconciles every gate to reported-only.
        let dir = git_repo("sc-git-blocked");
        let output = analyze(AnalyzeRequest {
            root: dir.clone(),
            repo: "blocked".into(),
            fail_on: Vec::new(),
            budget: Duration::from_secs(30),
            config: Config::default(),
            diff_base: None,
            diff_head: None,
            path_list: Vec::new(),
            spec_path: None,
            mutation_override: None,
            llm_override: None,
            intent: None,
        });
        assert!(!output.scorecard.git.dirty);
        assert_eq!(output.scorecard.git.head.as_ref().unwrap().len(), 40);
        assert!(output.scorecard.gates.iter().all(|gate| !gate.enforced));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn analyzed(rel: &str, imports: &[&str], local_names: &[&str]) -> crate::facts::AnalyzedFile {
        crate::facts::AnalyzedFile {
            rel: rel.to_string(),
            loc: 10,
            functions: Vec::new(),
            imports: imports
                .iter()
                .map(|name| sc_graph::ImportHit {
                    file: rel.to_string(),
                    crate_name: (*name).to_string(),
                    line: 1,
                })
                .collect(),
            perf: Vec::new(),
            items: Vec::new(),
            secrets: Vec::new(),
            local_names: local_names.iter().map(|name| (*name).to_string()).collect(),
        }
    }

    #[test]
    fn hallucinated_import_ignores_local_modules_and_aliases() {
        use crate::scope::Selection;
        let dir = std::env::temp_dir().join(format!("sc-sca-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let files = vec![
            // `mod score;` declared here, `pub use score::...` first segment.
            analyzed("src/lib.rs", &["score", "missing"], &["score", "renamed"]),
            // File module: src/score.rs is module `score` with no decl needed.
            analyzed("src/score.rs", &["renamed"], &[]),
        ];
        let selection = Selection {
            mode: "tree".into(),
            files,
            crap_functions: Vec::new(),
            new_symbols: std::collections::BTreeSet::new(),
            narrow_untested: false,
            paths: Vec::new(),
            loc_changed: 0,
            files_changed: 0,
            base: None,
        };
        let mut ran = Vec::new();
        let mut skipped = Vec::new();
        let mut findings = Vec::new();
        let count = import_findings(
            &dir,
            &selection,
            true,
            &mut ran,
            &mut skipped,
            &mut findings,
        );
        assert_eq!(count, 1);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].symbol.as_deref(), Some("missing"));
        assert_eq!(findings[0].rule, "sca.undeclared_dependency");
        assert!(findings[0].message.starts_with("Advisory:"));
        assert!(!findings[0].message.contains("Strongly"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hallucinated_import_allows_member_package_and_path_deps() {
        use crate::scope::Selection;
        let dir = std::env::temp_dir().join(format!("sc-sca-member-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("host")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers = [\"host\"]\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("host/Cargo.toml"),
            "[package]\nname = \"sc-core\"\nversion = \"0.1.0\"\n\n[dependencies]\nprismattyc-mux = { path = \"../mux\" }\nhtml5ever = \"0.1\"\n",
        )
        .unwrap();
        let files = vec![analyzed(
            "host/src/lib.rs",
            &["sc_core", "prismattyc_mux", "html5ever", "missing"],
            &[],
        )];
        let selection = Selection {
            mode: "tree".into(),
            files,
            crap_functions: Vec::new(),
            new_symbols: std::collections::BTreeSet::new(),
            narrow_untested: false,
            paths: Vec::new(),
            loc_changed: 0,
            files_changed: 0,
            base: None,
        };
        let mut findings = Vec::new();
        let count = import_findings(
            &dir,
            &selection,
            true,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut findings,
        );
        assert_eq!(count, 1, "{findings:?}");
        assert_eq!(findings[0].symbol.as_deref(), Some("missing"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_module_names_follow_rust_layout() {
        assert_eq!(file_module("src/score.rs").as_deref(), Some("score"));
        assert_eq!(file_module("src/foo/mod.rs").as_deref(), Some("foo"));
        assert_eq!(file_module("src/main.rs"), None);
        assert_eq!(file_module("src/lib.rs"), None);
        assert_eq!(file_module("src/mod.rs"), None);
        assert_eq!(file_module("README.md"), None);
    }

    #[test]
    fn crap_gate_reason_names_threshold_and_untested_cc() {
        assert_eq!(crap_gate_reason(1, 0, 15), "1 function over threshold");
        assert_eq!(
            crap_gate_reason(0, 2, 15),
            "2 functions at or above CC 15 with no coverage"
        );
    }

    #[test]
    fn sca_detail_names_each_measured_class() {
        assert_eq!(sca_detail(1, 0, 0), "1 undeclared dependency");
        assert_eq!(sca_detail(2, 0, 0), "2 undeclared dependencies");
        assert_eq!(
            sca_detail(2, 1, 0),
            "2 undeclared dependencies, 1 hallucinated import"
        );
        assert_eq!(
            sca_detail(0, 0, 1),
            "1 import not classified because the package index was not checked"
        );
        let gate = advisory_detail("sca", 1, &sca_detail(1, 0, 0));
        assert!(!gate.pass);
        assert!(!gate.enforced);
        assert_eq!(
            gate.reason.as_deref(),
            Some("1 undeclared dependency (advisory; does not fail the process)")
        );
    }
}

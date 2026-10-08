// SPDX-License-Identifier: MPL-2.0
//! Assemble a scorecard from the deterministic engines.
//!
//! Engines return structured findings. Nothing in this crate writes user-facing
//! diagnostics; the CLI formats the scorecard.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use sc_core::{
    apply_disposition, compute_scores, CrapSection, Finding, Gate, GitInfo, LlmSection, Metrics,
    MutationSection, RunRecord, Scope, Scorecard, SpecSection, SCORECARD_VERSION,
};
use sc_graph::FunctionInfo;

use crate::cargo_test::{targeted_test_names, test_findings};
use crate::clippy::{clippy_workspace, first_clippy_diagnostic};
use crate::command::{run_cargo, run_cmd, CommandError};
use crate::compile::{generic_compile_failure, parse_compiler_messages};
use crate::coverage::parse_coverage_json;
use crate::crap::{evaluate, unmatched_functions};

use crate::mutation::run_mutation;
use crate::scope::{empty_selection, Selection};
use crate::spec_check::{check_spec, gap_value};

/// Progress updates for a long analysis run (e.g. a CLI spinner).
///
/// Called with the current step label as each gate/pack command starts.
/// Callbacks must be cheap and non-blocking; they never affect the result.
pub type ProgressCallback = std::sync::Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Clone)]
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
    pub progress: Option<ProgressCallback>,
}

impl std::fmt::Debug for AnalyzeRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnalyzeRequest")
            .field("root", &self.root)
            .field("repo", &self.repo)
            .field("fail_on", &self.fail_on)
            .field("budget", &self.budget)
            .field("config", &self.config)
            .field("diff_base", &self.diff_base)
            .field("diff_head", &self.diff_head)
            .field("path_list", &self.path_list)
            .field("spec_path", &self.spec_path)
            .field("mutation_override", &self.mutation_override)
            .field("llm_override", &self.llm_override)
            .field("intent", &self.intent)
            .field("progress", &self.progress.as_ref().map(|_| "..."))
            .finish()
    }
}

/// Report the current analysis step to the request's progress callback, if any.
fn report(request: &AnalyzeRequest, step: &str) {
    if let Some(progress) = &request.progress {
        progress(step);
    }
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
    analyze_with_generated_paths(request, &[])
}

pub fn analyze_with_generated_paths(
    request: AnalyzeRequest,
    generated_paths: &[PathBuf],
) -> AnalyzeOutput {
    // Snapshot Git state before any engine runs and exclude Scorecard's saved
    // report and requested output files from earlier runs.
    report(&request, "Detecting pack");
    let git = git_info(&request.root, generated_paths);
    match crate::pack::detect_with_generated(
        &request.root,
        &request.config.pack,
        &request.config.scope.include_generated,
    ) {
        Ok(crate::pack::Detected::Pack(crate::pack::PackId::Rust)) => analyze_rust(request, git),
        Ok(crate::pack::Detected::Pack(crate::pack::PackId::Python)) => {
            analyze_python(request, git)
        }
        Ok(crate::pack::Detected::Pack(crate::pack::PackId::Web)) => analyze_web(request, git),
        Ok(crate::pack::Detected::Pack(pack)) => analyze_unsupported(request, pack, git),
        Ok(crate::pack::Detected::Unknown) => analyze_blocked(
            request,
            "unknown",
            "no language pack detected; set pack in analyzer.toml or pass --pack",
            git,
        ),
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
    report(&request, "Writing report");
    finish(Draft {
        root: request.root.clone(),
        repo: request.repo,
        pack: pack.to_string(),
        test_selection: "full-suite".into(),
        git,
        mode: "tree".into(),
        paths: Vec::new(),
        base: None,
        other_paths: None,
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
        generated_files_warning: None,
        runs: Vec::new(),
        analyzer_error: true,
    })
}

fn analyze_web(request: AnalyzeRequest, git: GitInfo) -> AnalyzeOutput {
    report(&request, "Checking markup");
    let mut findings = Vec::new();
    let source_scan = crate::poly_cc::scan_for_pack(
        &request.root,
        "node",
        &request.config.scope.exclude,
        &request.config.scope.include_generated,
    );
    let mut functions = source_scan.functions;
    let html = html_files(
        &request.root,
        &request.config.scope.exclude,
        &request.config.scope.include_generated,
    );
    for rel in &html {
        let Ok(text) = std::fs::read_to_string(request.root.join(rel)) else {
            continue;
        };
        let (html, parsed) = crate::html_doc::html_findings(rel, &text);
        findings.extend(html);
        findings.extend(crate::links::link_findings(
            &request.root,
            rel,
            &parsed.elements,
        ));
        findings.extend(crate::a11y::check_elements(
            rel,
            &parsed.elements,
            &request.config.a11y.disable,
            true,
        ));
        for script in parsed.scripts {
            functions.extend(crate::poly_cc::javascript_in(
                rel,
                &script.body,
                script.line.saturating_sub(1),
            ));
        }
    }
    report(&request, "Checking secrets");
    let secrets = crate::pack::text_secrets_with_generated(
        &request.root,
        &request.config.scope.exclude,
        &request.config.scope.include_generated,
    );
    let secret_errors = secrets
        .iter()
        .filter(|finding| finding.severity == "error")
        .count();
    findings.extend(secrets);
    report(&request, "Scoring complexity (crap)");
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
            "Run JavaScript tests with c8 or nyc, then re-run `sc analyze`",
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
    if !fail_on.is_empty() {
        if html_enforced && !fail_on.iter().any(|gate| gate == "html") {
            fail_on.push("html".into());
        }
        if links_enforced && !fail_on.iter().any(|gate| gate == "links") {
            fail_on.push("links".into());
        }
        if a11y_enforced && !fail_on.iter().any(|gate| gate == "a11y") {
            fail_on.push("a11y".into());
        }
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
    report(&request, "Writing report");
    finish(Draft {
        root: request.root.clone(),
        repo: request.repo,
        pack: "web".into(),
        test_selection: "full-suite".into(),
        git,
        mode: "tree".into(),
        paths: Vec::new(),
        base: None,
        other_paths: None,
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
            files_changed: html.len() as u64,
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
        generated_files_warning: sc_core::GeneratedFilesWarning::from_paths(
            generated_files_warning(
                &request.root,
                &request.config.scope.exclude,
                &request.config.scope.include_generated,
            ),
        ),
        runs: Vec::new(),
        analyzer_error: false,
    })
}

fn markup_files(
    root: &Path,
    exts: &[&str],
    exclude: &[String],
    include_generated: &[String],
) -> Vec<String> {
    let mut files: Vec<String> =
        sc_graph::walk_files(root, root, exclude, include_generated, Some(6))
            .into_iter()
            .filter(|path| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| exts.contains(&ext))
            })
            .filter_map(|path| {
                path.strip_prefix(root)
                    .ok()
                    .map(|rel| rel.to_string_lossy().replace('\\', "/"))
            })
            .collect();
    files.sort();
    files.dedup();
    files.truncate(200);
    files
}

fn generated_files_warning(
    root: &Path,
    exclude: &[String],
    include_generated: &[String],
) -> Vec<String> {
    let paths: Vec<String> = sc_graph::walk_files(root, root, exclude, include_generated, None)
        .into_iter()
        .filter_map(|path| {
            path.strip_prefix(root)
                .ok()
                .map(|rel| rel.to_string_lossy().replace('\\', "/"))
        })
        .collect();
    sc_graph::analyzed_generated_files(root, &paths, include_generated)
}

fn html_files(root: &Path, exclude: &[String], include_generated: &[String]) -> Vec<String> {
    markup_files(root, &["html", "htm"], exclude, include_generated)
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

/// Coverage may only blame the tests gate when the test command actually ran.
fn tests_ran_and_failed(ran: &[String], gates: &[Gate]) -> bool {
    ran.iter().any(|engine| engine == "tests")
        && gates.iter().any(|gate| gate.id == "tests" && !gate.pass)
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
        Some(clippy_workspace(
            request.config.commands.lint.trim(),
            crate::facts::is_workspace_root(&request.root, &request.config.toolchain),
        ))
    };
    let started = crate::pack_cov::run_start();
    crate::pack_cov::clear(&request.root);
    report(&request, "Running tests");
    let tools = crate::toolchain::run_with_generated(
        pack,
        &request.root,
        Instant::now() + request.budget,
        user_lint.as_deref(),
        request.config.engines.coverage,
        &request.config.scope.exclude,
        &request.config.scope.include_generated,
    );
    let mut findings = tools.findings;
    findings.push(unavailable(
        "sca",
        "dependency check is not implemented for this pack",
    ));
    report(&request, "Checking secrets");
    let secrets = crate::pack::text_secrets_with_generated(
        &request.root,
        &request.config.scope.exclude,
        &request.config.scope.include_generated,
    );
    let secret_errors = secrets
        .iter()
        .filter(|finding| finding.severity == "error")
        .count();
    findings.extend(secrets);
    let jsx_files = if pack == crate::pack::PackId::Node {
        markup_files(
            &request.root,
            &["jsx", "tsx"],
            &request.config.scope.exclude,
            &request.config.scope.include_generated,
        )
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
        if !fail_on.is_empty() && enforced && !fail_on.iter().any(|gate| gate == "a11y") {
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
    let scan = crate::poly_cc::scan_for_pack(
        &request.root,
        pack.as_str(),
        &request.config.scope.exclude,
        &request.config.scope.include_generated,
    );
    let functions = scan.functions;
    let known = crate::poly_cc::coverage_paths_with_scope(
        &request.root,
        pack.as_str(),
        &request.config.scope.exclude,
        &request.config.scope.include_generated,
    );
    let coverage = if pack == crate::pack::PackId::Go {
        std::fs::read_to_string(crate::toolchain::go_cover_path(&request.root))
            .ok()
            .map(|text| crate::poly_cc::go_coverage(&text, &functions, &known))
    } else {
        crate::pack_cov::load(pack.as_str(), &request.root, &functions, started)
    };
    report(&request, "Scoring complexity (crap)");
    let crap = crate::crap::evaluate(
        &functions,
        coverage.as_ref(),
        request.config.gates.crap_threshold,
        request.config.gates.new_fn_untested_cc,
        |_| true,
    );
    let tests_failed = tests_ran_and_failed(&ran, &gates);
    let test_exit = runs
        .iter()
        .rev()
        .find(|run| run.engine == "tests" && run.exit_code != Some(0))
        .and_then(|run| run.exit_code);
    let tests_reason = gates
        .iter()
        .find(|gate| gate.id == "tests")
        .and_then(|gate| gate.reason.as_deref())
        .unwrap_or("");
    if !crap.coverage_complete {
        if coverage.is_none() && tests_failed {
            let reason = match test_exit {
                Some(code) => {
                    format!("coverage skipped: tests gate failed (test command exit code {code})")
                }
                None if !tests_reason.is_empty() => {
                    format!("coverage skipped: tests gate failed ({tests_reason})")
                }
                None => "coverage skipped: tests gate failed (test command was not run)".into(),
            };
            findings.push(crate::coverage::missing_finding(
                &reason,
                "Fix the failing tests and re-run to collect coverage",
            ));
        } else {
            let unmatched = coverage
                .as_ref()
                .map(|data| crate::crap::unmatched_count(&functions, data))
                .unwrap_or(functions.len() as u64);
            if unmatched > 0 {
                let fix = if !request.config.engines.coverage {
                    "Set `engines.coverage = true` in analyzer.toml and re-run `sc analyze`"
                } else {
                    coverage_fix_hint(pack)
                };
                findings.push(crate::coverage::missing_finding(
                    &format!("coverage data is missing for {unmatched} analyzed function(s)"),
                    fix,
                ));
            }
        }
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
    report(&request, "Writing report");
    finish(Draft {
        root: request.root.clone(),
        repo: request.repo,
        pack: pack.as_str().to_string(),
        test_selection: pack.test_selection().into(),
        git,
        mode: "tree".into(),
        paths: Vec::new(),
        base: None,
        other_paths: None,
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
        generated_files_warning: sc_core::GeneratedFilesWarning::from_paths(
            generated_files_warning(
                &request.root,
                &request.config.scope.exclude,
                &request.config.scope.include_generated,
            ),
        ),
        runs,
        analyzer_error: false,
    })
}

struct PythonScope {
    py: Option<BTreeSet<String>>,
    files: Option<BTreeSet<String>>,
    base: Option<String>,
    other_paths: Option<u64>,
    loc: u64,
    files_changed: u64,
}

fn python_scope(request: &AnalyzeRequest) -> Result<PythonScope, String> {
    let Some(requested) = request.diff_base.as_deref() else {
        return Ok(PythonScope {
            py: None,
            files: None,
            base: None,
            other_paths: None,
            loc: 0,
            files_changed: 0,
        });
    };
    if !request.path_list.is_empty() {
        return Err("pass either --diff or --paths, not both".into());
    }
    let base = crate::scope::resolve_base(&request.root, requested)?;
    let rels = crate::scope::changed_rels(&request.root, &base, request.diff_head.as_deref())?;
    let files: BTreeSet<String> = rels.iter().cloned().collect();
    let py: BTreeSet<String> = rels
        .into_iter()
        .filter(|rel| rel.ends_with(".py"))
        .collect();
    let tree = crate::python::python_rels(&request.root);
    let other = tree.iter().filter(|rel| !py.contains(*rel)).count() as u64;
    let loc = crate::scope::added_lines(&request.root, &base, request.diff_head.as_deref(), &py);
    let files_changed = py.len() as u64;
    Ok(PythonScope {
        py: Some(py),
        files: Some(files),
        base: Some(base),
        other_paths: Some(other),
        loc,
        files_changed,
    })
}

fn analyze_python(request: AnalyzeRequest, git: GitInfo) -> AnalyzeOutput {
    let scoped = match python_scope(&request) {
        Ok(scoped) => scoped,
        Err(err) => return python_scope_error(request, git, err),
    };
    let deadline = Instant::now() + request.budget;
    let started = crate::pack_cov::run_start();
    crate::pack_cov::clear(&request.root);
    report(&request, "Running tests");
    let outcome = crate::python::run_with_generated(
        &request.root,
        deadline,
        request.config.gates.crap_threshold,
        request.config.gates.new_fn_untested_cc,
        started,
        crate::scope::ScanScope {
            exclude: &request.config.scope.exclude,
            include_generated: &request.config.scope.include_generated,
        },
        request.config.engines.coverage,
        scoped.py.as_ref(),
        scoped.files.as_ref(),
    );
    let gates = vec![
        if outcome.checks.types.pass {
            gate("types", true, "")
        } else {
            gate("types", false, &outcome.checks.types.reason)
        },
        tests_gate(
            outcome.checks.tests.enforced,
            outcome.checks.tests.pass,
            &outcome.checks.tests.reason,
        ),
        crap_gate(
            outcome.crap.coverage_complete,
            outcome.crap.over,
            outcome.crap.untested,
            request.config.gates.new_fn_untested_cc,
        ),
        advisory_detail(
            "sca",
            outcome.checks.sca.total(),
            &sca_detail(
                outcome.checks.sca.undeclared,
                outcome.checks.sca.hallucinated,
                outcome.checks.sca.unresolved,
            ),
        ),
        gate(
            "lint",
            outcome.checks.lint.pass,
            &outcome.checks.lint.reason,
        ),
        gate(
            "secrets",
            outcome.checks.secret_errors == 0,
            &if outcome.checks.secret_errors == 0 {
                String::new()
            } else {
                format!("{} secrets", outcome.checks.secret_errors)
            },
        ),
    ];
    report(&request, "Writing report");
    let diff = scoped.py.is_some();
    let paths: Vec<String> = scoped.py.unwrap_or_default().into_iter().collect();
    finish(Draft {
        root: request.root.clone(),
        repo: request.repo,
        pack: "python".into(),
        test_selection: if diff {
            "diff-tests".into()
        } else {
            "full-suite".into()
        },
        git,
        mode: if diff { "diff".into() } else { "tree".into() },
        paths,
        base: scoped.base,
        other_paths: scoped.other_paths,
        fail_on: request.fail_on,
        findings: outcome.findings,
        ran: outcome.ran,
        skipped: outcome.skipped,
        gates,
        metrics: sc_core::Metrics {
            loc_changed: scoped.loc,
            files_changed: scoped.files_changed,
            coverage_changed: 0.0,
            crap_max: outcome.crap.crap_max,
            crap_over_threshold: outcome.crap.over,
            hallucinated_imports: outcome.checks.sca.hallucinated,
            undeclared_dependencies: outcome.checks.sca.undeclared,
        },
        threshold: request.config.gates.crap_threshold,
        worst: outcome.crap.worst,
        mutation: MutationSection::skipped(),
        spec: SpecSection::empty(),
        intent: request.intent.clone(),
        llm: other_pack_llm(request.llm_override.unwrap_or(request.config.llm.enabled)),
        generated_files_warning: sc_core::GeneratedFilesWarning::from_paths(
            generated_files_warning(
                &request.root,
                &request.config.scope.exclude,
                &request.config.scope.include_generated,
            ),
        ),
        runs: outcome.runs,
        analyzer_error: false,
    })
}

fn python_scope_error(request: AnalyzeRequest, git: GitInfo, err: String) -> AnalyzeOutput {
    let findings = vec![unavailable("scope", &err)];
    let gates = vec![
        gate("types", false, &err),
        tests_gate(true, false, &err),
        gate("crap", false, &err),
        advisory_detail("sca", 0, ""),
        gate("lint", false, &err),
        gate("secrets", false, &err),
    ];
    report(&request, "Writing report");
    finish(Draft {
        root: request.root.clone(),
        repo: request.repo,
        pack: "python".into(),
        test_selection: "diff-tests".into(),
        git,
        mode: "diff".into(),
        paths: Vec::new(),
        base: None,
        other_paths: None,
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
            "secrets".into(),
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
        generated_files_warning: None,
        runs: Vec::new(),
        analyzer_error: true,
    })
}

fn analyze_rust(request: AnalyzeRequest, git: GitInfo) -> AnalyzeOutput {
    let deadline = Instant::now() + request.budget;
    let root = &request.root;
    let threshold = request.config.gates.crap_threshold;
    let (selection, select_error) = match crate::scope::select_with_generated(
        root,
        crate::scope::ScanScope {
            exclude: &request.config.scope.exclude,
            include_generated: &request.config.scope.include_generated,
        },
        request.diff_base.as_deref(),
        request.diff_head.as_deref(),
        &request.path_list,
        &request.config.toolchain,
        request.config.engines.perf,
    ) {
        Ok(selection) => (selection, None),
        Err(err) => (empty_selection(), Some(err)),
    };

    let workspace_root = if select_error.is_some() {
        crate::facts::is_workspace_root(root, &request.config.toolchain)
    } else {
        selection.workspace_root
    };
    let mut state = RustState::new(workspace_root, request.config.toolchain.clone());
    if let Some(err) = &select_error {
        state.findings.push(unavailable("scope", err));
    }
    state.analyzer_error = select_error.is_some();

    let manifest = root.join("Cargo.toml");
    if !manifest.is_file() {
        missing_manifest(&mut state);
    } else {
        report(&request, "Checking types (cargo check)");
        compile_phase(root, &manifest, deadline, &mut state);

        report(&request, "Running tests (cargo test)");
        test_phase(root, &selection, &manifest, deadline, &mut state);
        report(&request, "Measuring coverage");
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
    workspace_root: bool,
    /// From `analyzer.toml` `toolchain` (empty means use the project file pin).
    toolchain_pin: String,
}

impl RustState {
    fn new(workspace_root: bool, toolchain_pin: String) -> Self {
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
            workspace_root,
            toolchain_pin,
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
    match run_check(
        root,
        manifest,
        deadline,
        state.workspace_root,
        &state.toolchain_pin,
    ) {
        Ok(captured) => {
            note_run(
                &mut state.runs,
                "compile",
                &cargo_gate_command("check", state.workspace_root, "--message-format=json"),
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
                &cargo_gate_command("check", state.workspace_root, "--message-format=json"),
                None,
                Duration::ZERO,
            );
            state
                .findings
                .push(timed_out("compile", "cargo check timed out"));
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
    match run_test_set(
        root,
        manifest,
        deadline,
        state.workspace_root,
        &targeted,
        &state.toolchain_pin,
        &mut state.runs,
    ) {
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
                .push(timed_out("tests", "cargo test timed out"));
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
        skip_coverage_when_tests_failed(state);
        return;
    }
    if !coverage_enabled {
        skip_engine(&mut state.skipped, "coverage");
        return;
    }
    match run_coverage(
        root,
        manifest,
        deadline,
        state.workspace_root,
        &state.toolchain_pin,
        &mut state.runs,
    ) {
        Ok(data) => record_coverage_success(selection, data, state),
        Err(err) => record_coverage_tool_missing(err, state),
    }
}

fn skip_engine(skipped: &mut Vec<String>, engine: &str) {
    if !skipped.iter().any(|name| name == engine) {
        skipped.push(engine.into());
    }
}

/// Coverage is skipped when tests did not pass. Only blame the tests gate when
/// the test command actually ran.
fn skip_coverage_when_tests_failed(state: &mut RustState) {
    skip_engine(&mut state.skipped, "coverage");
    if !state.ran.iter().any(|engine| engine == "tests") {
        return;
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
    state.findings.push(crate::coverage::missing_finding(
        &reason,
        "Fix the failing tests and re-run to collect coverage",
    ));
}

fn record_coverage_success(
    selection: &Selection,
    data: crate::coverage::CoverageData,
    state: &mut RustState,
) {
    state.ran.push("coverage".into());
    let unmatched = unmatched_functions(&selection.crap_functions, &data);
    if !unmatched.is_empty() {
        state.findings.push(unmatched_coverage_finding(&unmatched));
    }
    state.line_rate = data.line_rate;
    state.coverage_data = Some(data);
}

fn unmatched_coverage_finding(unmatched: &[&FunctionInfo]) -> Finding {
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
        suggested_action: Some("Check that the function is compiled into the test binary".into()),
        disposition: String::new(),
    };
    if let [function] = unmatched {
        finding.file = function.file.clone();
        finding.symbol = Some(function.symbol.clone());
        finding.span = Some(function.span.clone());
    }
    finding
}

fn record_coverage_tool_missing(err: String, state: &mut RustState) {
    state.skipped.push("coverage".into());
    let detail = rust_coverage_detail(&err);
    state.findings.push(crate::coverage::missing_finding(
        &format!("coverage tooling unavailable: {}", detail.reason),
        detail.fix,
    ));
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
    report(request, "Running lint (cargo clippy)");
    let report_lint = run_lint_engine(
        root,
        &request.config.commands.lint,
        state.workspace_root,
        state.types_pass,
        deadline,
        &request.config.toolchain,
        LintSink {
            ran: &mut state.ran,
            skipped: &mut state.skipped,
            findings: &mut state.findings,
            runs: &mut state.runs,
        },
    );

    let untested_cc = request.config.gates.new_fn_untested_cc;
    report(request, "Scoring complexity (crap)");
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
            "Install `cargo-llvm-cov` and run `sc analyze` again",
        ));
    }
    let crap_gate = crap_gate(
        crap.coverage_complete,
        crap.over,
        crap.untested,
        untested_cc,
    );
    state.findings.extend(crap.findings);

    report(request, "Checking dependencies");
    let undeclared = import_findings(
        root,
        selection,
        request.config.engines.sca,
        &mut state.ran,
        &mut state.skipped,
        &mut state.findings,
    );
    report(request, "Checking secrets");
    secret_and_perf(
        selection,
        root,
        &request.config.scope.exclude,
        &request.config.scope.include_generated,
        &mut state.ran,
        &mut state.findings,
    );
    report(request, "Checking spec");
    let mut spec = spec_engine(
        request,
        selection,
        &mut state.ran,
        &mut state.skipped,
        &mut state.findings,
    );
    report(request, "Running mutation");
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

    report(request, "Writing report");
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
        other_paths: selection.other_paths,
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
        generated_files_warning: sc_core::GeneratedFilesWarning::from_paths(
            generated_files_warning(
                root,
                &request.config.scope.exclude,
                &request.config.scope.include_generated,
            ),
        ),
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
    other_paths: Option<u64>,
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
    generated_files_warning: Option<sc_core::GeneratedFilesWarning>,
    runs: Vec<RunRecord>,
    analyzer_error: bool,
}

fn finish(mut draft: Draft) -> AnalyzeOutput {
    order_engines(&mut draft.ran, &mut draft.skipped);
    sort_findings(&mut draft.findings);
    unique_ids(&mut draft.findings);
    apply_disposition(&mut draft.findings);
    let verdict_failed = sc_core::verdict_fails(&draft.gates);
    let exit_failed = sc_core::exit_code_fails(&draft.gates, &draft.fail_on);
    let scores = compute_scores(&draft.findings);
    let scorecard = Scorecard {
        identity: sc_core::ScorecardIdentity {
            version: SCORECARD_VERSION.to_string(),
            id: sc_core::new_scorecard_id(),
            repo: draft.repo,
            pack: draft.pack,
            test_selection: draft.test_selection,
        },
        context: sc_core::ScorecardContext {
            git: draft.git,
            intent: draft.intent,
            scope: Scope {
                mode: draft.mode,
                paths: draft.paths,
                base: draft.base,
                other_paths: draft.other_paths,
            },
        },
        verdict: if draft.analyzer_error || verdict_failed {
            "fail"
        } else {
            "pass"
        }
        .to_string(),
        fail_on: draft.fail_on,
        engines: sc_core::ScorecardEngines {
            engines_run: draft.ran,
            engines_skipped: draft.skipped,
        },
        measures: sc_core::ScorecardMeasures {
            scores,
            gates: draft.gates,
            metrics: draft.metrics,
        },
        sections: sc_core::ScorecardSections {
            crap: CrapSection {
                threshold: draft.threshold,
                worst: draft.worst,
            },
            mutation: draft.mutation,
            findings: draft.findings,
            generated_files_warning: draft.generated_files_warning,
            spec: draft.spec,
            llm: draft.llm,
        },
        runs: draft.runs,
    };
    write_last_scorecard(&draft.root, &scorecard);
    let status = if draft.analyzer_error {
        RunStatus::AnalyzerError
    } else if exit_failed {
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
    workspace_root: bool,
    names: &[String],
    toolchain_pin: &str,
    runs: &mut Vec<RunRecord>,
) -> Result<TestCapture, CommandError> {
    if names.is_empty() {
        let started = Instant::now();
        let result = run_tests(root, manifest, deadline, workspace_root, toolchain_pin);
        match &result {
            Ok(captured) => note_run(
                runs,
                "tests",
                &cargo_gate_command("test", workspace_root, ""),
                captured.status.code(),
                captured.elapsed,
            ),
            Err(_) => note_run(
                runs,
                "tests",
                &cargo_gate_command("test", workspace_root, ""),
                None,
                started.elapsed(),
            ),
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
        let command = format!("{} {name}", cargo_gate_command("test", workspace_root, ""));
        let result = run_one_test(
            root,
            manifest,
            deadline,
            workspace_root,
            name,
            toolchain_pin,
        );
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
    workspace_root: bool,
    name: &str,
    toolchain_pin: &str,
) -> Result<crate::command::Captured, CommandError> {
    let manifest = manifest.to_string_lossy().to_string();
    let mut args = vec!["test"];
    add_workspace_arg(&mut args, workspace_root);
    args.extend([name, "--manifest-path", &manifest, "--color", "never"]);
    run_cargo(root, &args, deadline, toolchain_pin)
}

struct LintSink<'a> {
    ran: &'a mut Vec<String>,
    skipped: &'a mut Vec<String>,
    findings: &'a mut Vec<Finding>,
    runs: &'a mut Vec<RunRecord>,
}

fn add_workspace_arg(args: &mut Vec<&str>, workspace_root: bool) {
    if workspace_root {
        args.push("--workspace");
    }
}

fn cargo_gate_command(action: &str, workspace_root: bool, suffix: &str) -> String {
    let workspace = if workspace_root { " --workspace" } else { "" };
    let suffix = if suffix.is_empty() {
        String::new()
    } else {
        format!(" {suffix}")
    };
    format!("cargo {action}{workspace}{suffix}")
}

fn run_lint_engine(
    root: &Path,
    lint: &str,
    workspace_root: bool,
    types_pass: bool,
    deadline: Instant,
    toolchain_pin: &str,
    sink: LintSink<'_>,
) -> bool {
    if !types_pass {
        sink.skipped.push("lint".into());
        return false;
    }
    let script = clippy_workspace(lint.trim(), workspace_root);
    if script.is_empty() {
        sink.skipped.push("lint".into());
        return false;
    }
    let script = script.as_str();
    let started = Instant::now();
    match run_shell(root, script, deadline, toolchain_pin) {
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
                let output = format!("{}\n{}", captured.stdout, captured.stderr);
                let diagnostic = first_clippy_diagnostic(root, &output);
                let detail = diagnostic
                    .as_ref()
                    .map(|item| item.message.clone())
                    .or_else(|| {
                        let brief = crate::command::brief(&output);
                        (!brief.is_empty()).then_some(brief)
                    });
                sink.findings.push(Finding {
                    id: "lint:failed".into(),
                    rule: "lint.failed".into(),
                    engine: "lint".into(),
                    severity: "error".into(),
                    file: diagnostic
                        .as_ref()
                        .map(|item| item.file.clone())
                        .unwrap_or_else(|| ".".into()),
                    span: diagnostic.as_ref().map(|item| sc_core::Span {
                        start_line: item.line,
                        start_col: item.column,
                        end_line: item.line,
                        end_col: item.column,
                    }),
                    symbol: diagnostic.as_ref().map(|item| item.lint.clone()),
                    message: match detail {
                        Some(detail) => format!("lint command failed: {detail}"),
                        None => "lint command failed".into(),
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
                .push(timed_out("lint", "lint command timed out"));
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
    text.contains("no such command") || text.contains("command not found")
}

fn run_shell(
    root: &Path,
    script: &str,
    deadline: Instant,
    toolchain_pin: &str,
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
    let policy = crate::rust_toolchain::resolve(root, toolchain_pin);
    crate::rust_toolchain::apply(&mut cmd, &policy);
    run_cmd(&mut cmd, timeout)
}

fn run_check(
    root: &Path,
    manifest: &Path,
    deadline: Instant,
    workspace_root: bool,
    toolchain_pin: &str,
) -> Result<crate::command::Captured, CommandError> {
    let manifest = manifest.to_string_lossy().to_string();
    let mut args = vec!["check"];
    add_workspace_arg(&mut args, workspace_root);
    args.extend([
        "--manifest-path",
        &manifest,
        "--message-format=json",
        "--color",
        "never",
    ]);
    run_cargo(root, &args, deadline, toolchain_pin)
}

fn run_tests(
    root: &Path,
    manifest: &Path,
    deadline: Instant,
    workspace_root: bool,
    toolchain_pin: &str,
) -> Result<crate::command::Captured, CommandError> {
    let manifest = manifest.to_string_lossy().to_string();
    let mut args = vec!["test"];
    add_workspace_arg(&mut args, workspace_root);
    args.extend(["--manifest-path", &manifest, "--color", "never"]);
    run_cargo(root, &args, deadline, toolchain_pin)
}

fn run_coverage(
    root: &Path,
    manifest: &Path,
    deadline: Instant,
    workspace_root: bool,
    toolchain_pin: &str,
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
    let mut args = vec!["llvm-cov"];
    add_workspace_arg(&mut args, workspace_root);
    args.extend([
        "--json",
        "--output-path",
        &out_s,
        "--manifest-path",
        &manifest_s,
    ]);
    let captured = run_cargo(root, &args, deadline, toolchain_pin);
    match &captured {
        Ok(captured) => note_run_with_budget(
            runs,
            "coverage",
            &cargo_gate_command("llvm-cov", workspace_root, "--json"),
            captured.status.code(),
            captured.elapsed,
            budget,
        ),
        Err(_) => note_run_with_budget(
            runs,
            "coverage",
            &cargo_gate_command("llvm-cov", workspace_root, "--json"),
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

struct CoverageDetail<'a> {
    reason: String,
    fix: &'a str,
}

fn rust_coverage_detail(error: &str) -> CoverageDetail<'static> {
    let lower = error.to_ascii_lowercase();
    if lower.contains("no such command")
        || lower.contains("llvm-cov is not installed")
        || lower.contains("unknown command: `llvm-cov`")
    {
        return CoverageDetail {
            reason: "cargo llvm-cov is not installed".into(),
            fix: "Install it with `cargo install cargo-llvm-cov`, then re-run `sc analyze`",
        };
    }
    CoverageDetail {
        reason: error.to_string(),
        fix: "Fix coverage tooling and re-run `sc analyze`",
    }
}

fn coverage_fix_hint(pack: crate::pack::PackId) -> &'static str {
    match pack {
        crate::pack::PackId::Node => {
            "Install c8 (`npm i -D c8`) or nyc (`npm i -D nyc`), then re-run `sc analyze`"
        }
        crate::pack::PackId::Go => "Run `go test -coverprofile=.sc/coverage/go.out ./...` and re-run `sc analyze`",
        crate::pack::PackId::Java => "Run tests with JaCoCo enabled and re-run `sc analyze`",
        crate::pack::PackId::CSharp => {
            "Run `dotnet test /p:CollectCoverage=true` and re-run `sc analyze`"
        }
        crate::pack::PackId::Php => {
            "Run phpunit with `--coverage-clover .sc/coverage/clover.xml` and re-run `sc analyze`"
        }
        crate::pack::PackId::Cpp => "Run tests with lcov output (`.sc/coverage/cpp.info`) and re-run `sc analyze`",
        crate::pack::PackId::Bash => {
            "Run the suite with kcov or cobertura XML output under `.sc/coverage`, then re-run `sc analyze`"
        }
        _ => "Enable this pack's coverage report and re-run `sc analyze`",
    }
}

fn git_info(root: &Path, generated_paths: &[PathBuf]) -> GitInfo {
    let head = run_git(root, &["rev-parse", "HEAD"]).ok().and_then(|text| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    });
    let analysis_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let report_path = analysis_root.join(".sc/last-scorecard.json");
    let cache_dir = analysis_root.join(".sc/cache");
    let coverage_dir = analysis_root.join(".sc/coverage");
    let status_root = run_git(root, &["rev-parse", "--show-toplevel"])
        .map(|path| PathBuf::from(path.trim()))
        .and_then(|path| std::fs::canonicalize(path).map_err(|_| ()))
        .unwrap_or_else(|_| analysis_root.clone());
    let generated_paths: Vec<PathBuf> = generated_paths
        .iter()
        .map(|path| canonical_git_path(&analysis_root, path))
        .collect();
    let dirty_paths: Vec<String> = run_git(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )
    .map(|status| {
        parse_git_status_paths(&status)
            .into_iter()
            .filter(|path| {
                // `git status` reports paths from the worktree root, even when
                // analysis starts in a project subdirectory.
                let status_path = normalize_git_path(&status_root.join(path));
                status_path != report_path
                    && !status_path.starts_with(&cache_dir)
                    && !status_path.starts_with(&coverage_dir)
                    && !generated_paths
                        .iter()
                        .any(|generated| generated == &status_path)
            })
            .collect()
    })
    .unwrap_or_default();
    let dirty = !dirty_paths.is_empty();
    GitInfo {
        head,
        dirty,
        dirty_paths,
    }
}

fn normalize_git_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn canonical_git_path(root: &Path, path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let normalized = normalize_git_path(&absolute);
    normalized
        .parent()
        .and_then(|parent| std::fs::canonicalize(parent).ok())
        .and_then(|parent| {
            normalized
                .file_name()
                .map(|file_name| parent.join(file_name))
        })
        .unwrap_or(normalized)
}

fn parse_git_status_paths(status: &str) -> Vec<String> {
    let mut records = status.split('\0').peekable();
    let mut paths = Vec::new();
    while let Some(record) = records.next() {
        let bytes = record.as_bytes();
        if bytes.len() < 4 {
            continue;
        }
        let code = &bytes[..2];
        let path = String::from_utf8_lossy(&bytes[3..]).into_owned();
        if !path.is_empty() {
            paths.push(path);
        }
        // Porcelain v1 -z adds the old path as a separate record for renames
        // and copies. The destination above is the useful changed path.
        if code.contains(&b'R') || code.contains(&b'C') {
            records.next();
        }
    }
    paths
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

fn timed_out(engine: &str, message: &str) -> Finding {
    let mut finding = unavailable(engine, message);
    finding.suggested_action = Some(crate::command::TIMEOUT_FIX.into());
    finding
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
    // A function with no coverage record is not scored. It must not hide a
    // measured failure, so the gate is advisory only when none failed.
    if !coverage_complete && over == 0 && untested == 0 {
        return Gate {
            id: "crap".into(),
            pass: false,
            enforced: false,
            reason: Some("some functions have no coverage record and were not scored".into()),
        };
    }
    let mut reason = crap_gate_reason(over, untested, untested_cc);
    if !coverage_complete {
        let note = "some functions have no coverage record and were not scored";
        if reason.is_empty() {
            reason = note.to_string();
        } else {
            reason = format!("{reason}; {note}");
        }
    }
    gate("crap", over == 0 && untested == 0, &reason)
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
        let allowed = crate::manifest::declared_for_file(root, &file.rel);
        count += undeclared_imports(file, &allowed, &local, findings);
    }
    count
}

fn undeclared_imports(
    file: &crate::facts::AnalyzedFile,
    allowed: &std::collections::BTreeSet<String>,
    local: &std::collections::BTreeSet<String>,
    findings: &mut Vec<Finding>,
) -> u64 {
    let mut count = 0u64;
    for import in &file.imports {
        let name = crate::manifest::normalize(&import.crate_name);
        if allowed.contains(&name) || local.contains(&name) {
            continue;
        }
        count += 1;
        let file_name = import.file.as_str();
        let crate_name = import.crate_name.as_str();
        findings.push(Finding {
            id: format!("sca:{file_name}:{crate_name}"),
            rule: "sca.undeclared_dependency".into(),
            engine: "sca".into(),
            severity: "warning".into(),
            file: file_name.to_string(),
            span: Some(sc_core::Span {
                start_line: import.line,
                start_col: 1,
                end_line: import.line,
                end_col: 1,
            }),
            symbol: Some(crate_name.to_string()),
            message: format!(
                "Advisory: crate `{crate_name}` is used in source and is not in this crate's Cargo.toml"
            ),
            evidence: serde_json::json!({"crate": crate_name}),
            suggested_action: Some(format!(
                "Add `{crate_name}` to this crate's Cargo.toml or remove the import"
            )),
            disposition: String::new(),
        });
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

fn secret_and_perf(
    selection: &Selection,
    root: &Path,
    exclude: &[String],
    include_generated: &[String],
    ran: &mut Vec<String>,
    findings: &mut Vec<Finding>,
) {
    ran.push("secrets".into());
    ran.push("perf".into());
    findings.extend(crate::pack::text_secrets_with_generated(
        root,
        exclude,
        include_generated,
    ));
    for file in &selection.files {
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
        &request.config.toolchain,
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
    let finished = finish_llm(backend, &request.config.llm.model, outcome);
    spec.section.llm_rounds = Some(finished.rounds);
    spec.section.gaps.extend(finished.gaps);
    if finished.ran {
        ran.push("llm".into());
    } else {
        skipped.push("llm".into());
    }
    findings.extend(finished.findings);
    Some(finished.section)
}

struct FinishedLlm {
    section: LlmSection,
    findings: Vec<Finding>,
    gaps: Vec<serde_json::Value>,
    ran: bool,
    rounds: u32,
}

fn finish_llm(backend: &str, configured_model: &str, outcome: sc_llm::LlmOutcome) -> FinishedLlm {
    let rounds = outcome.rounds;
    if let Some(reason) = outcome.skipped {
        let reason = explain_llm_failure(&reason);
        return FinishedLlm {
            section: LlmSection::skipped(reason.clone()),
            findings: vec![unavailable("llm", &reason)],
            gaps: Vec::new(),
            ran: false,
            rounds,
        };
    }
    let gap_count = outcome.gaps.len();
    let model = if outcome.model.is_empty() {
        configured_model.to_string()
    } else {
        outcome.model.clone()
    };
    let notes = outcome.notes;
    let mut gaps = Vec::new();
    let mut findings = Vec::new();
    for gap in outcome.gaps {
        let message = if gap.detail.is_empty() {
            gap.item.clone()
        } else {
            format!("{}: {}", gap.item, gap.detail)
        };
        let item = gap.item;
        let detail = gap.detail;
        gaps.push(serde_json::json!({
            "kind": "llm",
            "name": item,
            "message": message.clone(),
        }));
        findings.push(Finding {
            id: format!("spec:llm:{}", item.replace(' ', "_")),
            rule: "spec.llm_gap".into(),
            engine: "llm".into(),
            severity: "warning".into(),
            file: ".".into(),
            span: None,
            symbol: Some(item.clone()),
            message,
            evidence: serde_json::json!({"item": item, "detail": detail}),
            suggested_action: Some("Update the code or the spec".into()),
            disposition: String::new(),
        });
    }
    FinishedLlm {
        section: LlmSection::ran(backend, model, rounds, gap_count, notes),
        findings,
        gaps,
        ran: true,
        rounds,
    }
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
    let files: Vec<&str> = functions.iter().map(|item| item.file.as_str()).collect();
    let measured: Vec<f64> = functions
        .iter()
        .filter_map(|function| {
            coverage.for_function_known(&function.file, &function.symbol, &files)
        })
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

fn tests_gate(enforced: bool, pass: bool, reason: &str) -> Gate {
    if enforced {
        gate("tests", pass, reason)
    } else if !reason.is_empty() {
        Gate {
            id: "tests".into(),
            pass: false,
            enforced: false,
            reason: Some(reason.to_string()),
        }
    } else {
        gate_reported("tests")
    }
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
    use sc_core::{Config, Span};

    fn sample_fn(file: &str, symbol: &str, line: u32) -> FunctionInfo {
        FunctionInfo {
            file: file.into(),
            symbol: symbol.into(),
            span: Span {
                start_line: line,
                start_col: 1,
                end_line: line + 1,
                end_col: 2,
            },
            cc: 3,
        }
    }

    #[test]
    fn generated_warning_lists_opted_in_directories_and_marked_sources() {
        let root =
            std::env::temp_dir().join(format!("sc-generated-warning-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("build")).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("build/parser.rs"), "fn parse() {}\n").unwrap();
        std::fs::write(root.join("build/secret.txt"), "private output\n").unwrap();
        std::fs::write(
            root.join("src/generated.rs"),
            "// Code generated by parser; DO NOT EDIT.\nfn generated() {}\n",
        )
        .unwrap();
        let include = vec!["build/**".into(), "src/generated.rs".into()];
        let exclude = vec!["build/secret.txt".into()];

        assert_eq!(
            generated_files_warning(&root, &exclude, &include),
            ["build/parser.rs", "src/generated.rs"]
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn unmatched_coverage_finding_sets_location_for_one_function() {
        let function = sample_fn("src/lib.rs", "wide", 12);
        let finding = unmatched_coverage_finding(&[&function]);
        assert_eq!(finding.rule, "coverage.unmatched");
        assert_eq!(finding.file, "src/lib.rs");
        assert_eq!(finding.symbol.as_deref(), Some("wide"));
        assert_eq!(finding.span.as_ref().unwrap().start_line, 12);
        assert_eq!(finding.evidence["unmatched"], 1);
        assert_eq!(finding.evidence["functions"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn unmatched_coverage_finding_caps_listed_evidence() {
        let functions: Vec<FunctionInfo> = (0..(MAX_UNMATCHED_FUNCTION_EVIDENCE + 5))
            .map(|index| sample_fn("src/lib.rs", &format!("f{index}"), index as u32 + 1))
            .collect();
        let refs: Vec<&FunctionInfo> = functions.iter().collect();
        let finding = unmatched_coverage_finding(&refs);
        assert_eq!(finding.file, ".");
        assert!(finding.symbol.is_none());
        assert!(finding.span.is_none());
        assert_eq!(
            finding.evidence["unmatched"],
            MAX_UNMATCHED_FUNCTION_EVIDENCE + 5
        );
        assert_eq!(
            finding.evidence["functions"].as_array().unwrap().len(),
            MAX_UNMATCHED_FUNCTION_EVIDENCE
        );
        assert!(finding.message.contains(&format!(
            "{} analyzed function(s)",
            MAX_UNMATCHED_FUNCTION_EVIDENCE + 5
        )));
    }

    #[test]
    fn record_coverage_tool_missing_skips_and_names_the_fix() {
        let mut state = RustState::new(false, String::new());
        record_coverage_tool_missing("cargo: no such command `llvm-cov`".into(), &mut state);
        assert!(state.skipped.iter().any(|engine| engine == "coverage"));
        let finding = state
            .findings
            .iter()
            .find(|finding| finding.rule == "coverage.missing")
            .expect("coverage.missing");
        assert!(
            finding.message.contains("cargo llvm-cov is not installed"),
            "{}",
            finding.message
        );
        assert_eq!(
            finding.suggested_action.as_deref(),
            Some("Install it with `cargo install cargo-llvm-cov`, then re-run `sc analyze`")
        );
    }

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

    fn llm_case(backend: &str) -> (AnalyzeRequest, SpecRun) {
        let mut config = Config::default();
        config.llm.backend = backend.into();
        config.llm.endpoint = "http://127.0.0.1:1".into();
        config.llm.api_key_env = "SC_LLM_CRAP_KEY_UNSET".into();
        let request = AnalyzeRequest {
            root: std::env::temp_dir(),
            repo: "demo".into(),
            fail_on: Vec::new(),
            budget: Duration::from_secs(5),
            config,
            diff_base: None,
            diff_head: None,
            path_list: Vec::new(),
            spec_path: None,
            mutation_override: None,
            llm_override: Some(true),
            intent: Some("keep the header contrast".into()),
            progress: None,
        };
        let spec = SpecRun {
            section: sc_core::SpecSection::empty(),
            ran_gate: false,
            pass: true,
            reason: String::new(),
        };
        (request, spec)
    }

    fn run_llm(request: &AnalyzeRequest, spec: &mut SpecRun) -> Option<LlmSection> {
        let mut ran = Vec::new();
        let mut skipped = Vec::new();
        let mut findings = Vec::new();
        llm_engine(request, spec, &mut ran, &mut skipped, &mut findings, &[])
    }

    #[test]
    fn tests_ran_and_failed_ignores_reported_gates() {
        let reported = Gate {
            id: "tests".into(),
            pass: false,
            enforced: false,
            reason: Some("package.json has no test script".into()),
        };
        assert!(!tests_ran_and_failed(&[], std::slice::from_ref(&reported)));
        assert!(!tests_ran_and_failed(
            &["coverage".into()],
            std::slice::from_ref(&reported)
        ));
        let enforced_fail = Gate {
            id: "tests".into(),
            pass: false,
            enforced: true,
            reason: Some("test failures".into()),
        };
        assert!(!tests_ran_and_failed(
            &[],
            std::slice::from_ref(&enforced_fail)
        ));
        assert!(tests_ran_and_failed(
            &["tests".into()],
            std::slice::from_ref(&enforced_fail)
        ));
    }

    #[test]
    fn explain_llm_failure_names_the_fix_for_each_cause() {
        let down = explain_llm_failure("connection refused");
        assert!(down.contains("connection refused"));
        assert!(down.contains("http://127.0.0.1:11434/v1"));
        assert!(explain_llm_failure("timed out").contains("did not answer"));
        assert!(explain_llm_failure("tcp connect").contains("did not answer"));
        let kept = explain_llm_failure("set endpoint under [llm] in analyzer.toml");
        assert_eq!(kept, "set endpoint under [llm] in analyzer.toml");
        let other = explain_llm_failure("model returned an empty body");
        assert!(other.contains("model returned an empty body"));
        assert!(other.contains("--llm off"));
    }

    #[test]
    fn finish_llm_records_gaps_and_a_skipped_review() {
        let skipped = finish_llm(
            "ollama",
            "qwen2.5-coder",
            sc_llm::LlmOutcome {
                gaps: Vec::new(),
                notes: Vec::new(),
                skipped: Some("connection refused".into()),
                rounds: 1,
                model: String::new(),
            },
        );
        assert!(!skipped.ran);
        assert_eq!(skipped.rounds, 1);
        assert_eq!(skipped.section.status, "skipped");
        assert!(skipped.findings[0].message.contains("did not answer"));

        let recorded = finish_llm(
            "ollama",
            "configured-model",
            sc_llm::LlmOutcome {
                gaps: vec![
                    sc_llm::LlmGap {
                        item: "header contrast".into(),
                        detail: "fails at 320px".into(),
                    },
                    sc_llm::LlmGap {
                        item: "empty detail".into(),
                        detail: String::new(),
                    },
                ],
                notes: vec!["opened report.rs".into()],
                skipped: None,
                rounds: 2,
                model: String::new(),
            },
        );
        assert!(recorded.ran);
        assert_eq!(recorded.section.model.as_deref(), Some("configured-model"));
        assert_eq!(recorded.section.verdict.as_deref(), Some("gaps found"));
        assert_eq!(recorded.gaps.len(), 2);
        assert_eq!(recorded.findings[0].rule, "spec.llm_gap");
        assert!(recorded.findings[0].message.contains("fails at 320px"));
        assert_eq!(recorded.findings[1].message, "empty detail");
        assert!(recorded.findings[0].id.contains("header_contrast"));
    }

    #[test]
    fn llm_engine_skips_without_calling_a_model_when_the_request_is_incomplete() {
        let (mut request, mut spec) = llm_case("ollama");
        request.llm_override = Some(false);
        assert!(run_llm(&request, &mut spec).is_none());

        request.llm_override = Some(true);
        request.intent = None;
        let section = run_llm(&request, &mut spec).unwrap();
        assert_eq!(section.status, "skipped");
        assert!(section.reason.unwrap().contains("--intent"));

        let dir = std::env::temp_dir().join(format!("sc-llm-spec-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        request.spec_path = Some(dir.join("missing.md"));
        request.intent = Some("keep the header contrast".into());
        let section = run_llm(&request, &mut spec).unwrap();
        assert!(section.reason.unwrap().contains("--spec"));

        std::fs::write(dir.join("spec.md"), "The header wraps.\n").unwrap();
        request.spec_path = Some(dir.join("spec.md"));
        request.config.llm.backend = "nope".into();
        let section = run_llm(&request, &mut spec).unwrap();
        assert!(section.reason.unwrap().contains("unknown llm backend nope"));

        request.config.llm.backend = "openai-compatible".into();
        std::env::remove_var("SC_LLM_CRAP_KEY_UNSET");
        let section = run_llm(&request, &mut spec).unwrap();
        assert!(section
            .reason
            .unwrap()
            .contains("SC_LLM_CRAP_KEY_UNSET is not set"));

        request.config.llm.backend = "ollama".into();
        request.config.llm.endpoint = "http://127.0.0.1:1".into();
        let section = run_llm(&request, &mut spec).unwrap();
        let reason = section.reason.unwrap();
        assert!(reason.contains("did not answer"), "{reason}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_python_tree_without_a_suite_does_not_blame_the_pack() {
        let gate = tests_gate(false, true, crate::python::NO_PYTEST_SUITE);
        assert!(!gate.pass);
        assert!(!gate.enforced);
        let reason = gate.reason.unwrap();
        assert!(reason.contains("no pytest suite"));
        assert!(reason.contains("`sc analyze`"));
        assert!(!reason.contains("not provided"));
        let pack = tests_gate(false, true, "");
        assert_eq!(pack.reason.as_deref(), Some("not provided by this pack"));
    }

    #[test]
    fn lint_failures_report_the_first_clippy_location_and_name() {
        let root = std::env::temp_dir();
        let script = r#"printf '%s\n' 'Checking demo v0.1.0' 'warning: this redundant pattern should be removed' ' --> src/lib.rs:8:5' ' = note: `#[warn(clippy::redundant_pattern_matching)]` on by default' ' = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#redundant_pattern_matching' 'error: this unwrap is denied' ' --> src/denied.rs:12:7' ' = note: `#[deny(clippy::unwrap_used)]` on by default' ' = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#unwrap_used' >&2; exit 1"#;
        let mut ran = Vec::new();
        let mut skipped = Vec::new();
        let mut findings = Vec::new();
        let mut runs = Vec::new();

        run_lint_engine(
            &root,
            script,
            false,
            true,
            Instant::now() + Duration::from_secs(5),
            "",
            LintSink {
                ran: &mut ran,
                skipped: &mut skipped,
                findings: &mut findings,
                runs: &mut runs,
            },
        );

        let finding = findings.first().expect("lint failure finding");
        assert_eq!(finding.file, "src/denied.rs");
        assert_eq!(finding.span.as_ref().unwrap().start_line, 12);
        assert_eq!(finding.span.as_ref().unwrap().start_col, 7);
        assert_eq!(finding.symbol.as_deref(), Some("clippy::unwrap_used"));
        assert_eq!(
            finding.message,
            "lint command failed: this unwrap is denied"
        );

        let warning = first_clippy_diagnostic(
            &root,
            "warning: first warning\n --> src/warning.rs:4:2\n = note: `#[warn(clippy::needless_return)]` on by default",
        )
        .expect("first warning when no error diagnostic is available");
        assert_eq!(warning.file, "src/warning.rs");
        assert_eq!(warning.line, 4);
        assert_eq!(warning.column, 2);
        assert_eq!(warning.lint, "clippy::needless_return");
    }

    #[test]
    fn workspace_root_checks_every_member_but_member_analysis_stays_in_scope() {
        let root = std::env::temp_dir().join(format!("sc-ws-broken-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("crates/app/src")).unwrap();
        std::fs::create_dir_all(root.join("crates/broken/src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/app\", \"crates/broken\"]\nresolver = \"2\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("crates/app/Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("crates/app/src/lib.rs"),
            "pub fn ok() -> i32 { 1 }\n#[test]\nfn passes() { assert_eq!(ok(), 1); }\n",
        )
        .unwrap();
        std::fs::write(
            root.join("crates/broken/Cargo.toml"),
            "[package]\nname = \"broken\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(root.join("crates/broken/src/lib.rs"), "pub fn bad( {\n").unwrap();
        let mut config = Config::default();
        config.engines.coverage = false;
        config.commands.lint.clear();
        let output = analyze(AnalyzeRequest {
            root: root.clone(),
            repo: "ws".into(),
            fail_on: vec!["types".into(), "tests".into()],
            budget: Duration::from_secs(120),
            config,
            diff_base: None,
            diff_head: None,
            path_list: Vec::new(),
            spec_path: None,
            mutation_override: Some("off".into()),
            llm_override: Some(false),
            intent: None,
            progress: None,
        });
        let types = output
            .scorecard
            .measures
            .gates
            .iter()
            .find(|gate| gate.id == "types")
            .unwrap();
        assert!(!types.pass, "{:?}", output.scorecard.sections.findings);
        assert!(types.enforced);
        assert!(output.scorecard.runs.iter().any(|run| {
            run.command.contains("cargo check") && run.command.contains("--workspace")
        }));

        let analyze_command_lint = |root: PathBuf| {
            let mut config = Config::default();
            config.commands.lint = "cargo clippy --workspace --all-targets".into();
            analyze_unsupported(
                AnalyzeRequest {
                    root,
                    repo: "command-pack".into(),
                    fail_on: vec!["lint".into()],
                    budget: Duration::from_secs(120),
                    config,
                    diff_base: None,
                    diff_head: None,
                    path_list: Vec::new(),
                    spec_path: None,
                    mutation_override: Some("off".into()),
                    llm_override: Some(false),
                    intent: None,
                    progress: None,
                },
                crate::pack::PackId::Command,
                GitInfo {
                    head: None,
                    dirty: true,
                    dirty_paths: Vec::new(),
                },
            )
        };
        let command_root = analyze_command_lint(root.clone());
        assert!(command_root
            .scorecard
            .runs
            .iter()
            .any(|run| { run.engine == "lint" && run.command.contains("--workspace") }));
        assert!(
            !command_root
                .scorecard
                .measures
                .gates
                .iter()
                .find(|gate| gate.id == "lint")
                .unwrap()
                .pass
        );

        let _ = std::fs::remove_dir_all(root.join("target"));

        let member = root.join("crates/app");
        let command_member = analyze_command_lint(member.clone());
        assert!(command_member
            .scorecard
            .runs
            .iter()
            .any(|run| { run.engine == "lint" && run.command == "cargo clippy --all-targets" }));
        assert!(
            command_member
                .scorecard
                .measures
                .gates
                .iter()
                .find(|gate| gate.id == "lint")
                .unwrap()
                .pass
        );

        let mut config = Config::default();
        config.engines.coverage = false;
        config.commands.lint.clear();
        let output = analyze(AnalyzeRequest {
            root: member.clone(),
            repo: "member".into(),
            fail_on: vec!["types".into(), "tests".into()],
            budget: Duration::from_secs(120),
            config,
            diff_base: None,
            diff_head: None,
            path_list: Vec::new(),
            spec_path: None,
            mutation_override: Some("off".into()),
            llm_override: Some(false),
            intent: None,
            progress: None,
        });
        let types = output
            .scorecard
            .measures
            .gates
            .iter()
            .find(|gate| gate.id == "types")
            .unwrap();
        assert!(types.pass, "{:?}", output.scorecard.sections.findings);
        let tests = output
            .scorecard
            .measures
            .gates
            .iter()
            .find(|gate| gate.id == "tests")
            .unwrap();
        assert!(tests.pass, "{:?}", output.scorecard.sections.findings);
        assert!(output.scorecard.runs.iter().any(|run| {
            run.engine == "compile"
                && run.command == "cargo check --message-format=json"
                && run.exit_code == Some(0)
        }));
        assert!(output.scorecard.runs.iter().any(|run| {
            run.engine == "tests" && run.command == "cargo test" && run.exit_code == Some(0)
        }));
        assert_eq!(output.scorecard.context.scope.paths, vec!["src/lib.rs"]);
        assert!(!output
            .scorecard
            .runs
            .iter()
            .any(|run| { run.command.contains("--workspace") }));
        let _ = std::fs::remove_dir_all(&root);

        let lone = std::env::temp_dir().join(format!("sc-lone-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&lone);
        std::fs::create_dir_all(lone.join("src")).unwrap();
        std::fs::write(
            lone.join("Cargo.toml"),
            "[package]\nname = \"lone\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            lone.join("src/lib.rs"),
            "pub fn ok() -> i32 { 1 }\n#[test]\nfn passes() { assert_eq!(ok(), 1); }\n",
        )
        .unwrap();
        let mut config = Config::default();
        config.engines.coverage = false;
        config.commands.lint.clear();
        let output = analyze(AnalyzeRequest {
            root: lone.clone(),
            repo: "lone".into(),
            fail_on: vec!["types".into(), "tests".into()],
            budget: Duration::from_secs(120),
            config,
            diff_base: None,
            diff_head: None,
            path_list: Vec::new(),
            spec_path: None,
            mutation_override: Some("off".into()),
            llm_override: Some(false),
            intent: None,
            progress: None,
        });
        let types = output
            .scorecard
            .measures
            .gates
            .iter()
            .find(|gate| gate.id == "types")
            .unwrap();
        assert!(types.pass, "{:?}", output.scorecard.sections.findings);
        let tests = output
            .scorecard
            .measures
            .gates
            .iter()
            .find(|gate| gate.id == "tests")
            .unwrap();
        assert!(tests.pass, "{:?}", output.scorecard.sections.findings);
        assert!(output.scorecard.runs.iter().any(|run| {
            run.command.contains("cargo check") && run.command.contains("--workspace")
        }));
        let _ = std::fs::remove_dir_all(&lone);
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
            progress: None,
        });
        assert_eq!(output.status, RunStatus::AnalyzerError);
        assert_eq!(output.scorecard.verdict, "fail");
        assert!(output.scorecard.sections.findings.iter().any(|finding| {
            finding.rule == "engine.unavailable"
                && finding.message.contains("--pack")
                && finding.message.contains("analyzer.toml")
        }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_python_file_and_one_header_stays_python() {
        let dir = std::env::temp_dir().join(format!("sc-header-tie-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pyproject.toml"), "[project]\nname = \"d\"\n").unwrap();
        std::fs::write(dir.join("app.py"), "def value():\n    return 1\n").unwrap();
        std::fs::write(dir.join("ext.h"), "int marker;\n").unwrap();
        assert_eq!(
            crate::pack::detect(&dir, "").unwrap(),
            crate::pack::Detected::Pack(crate::pack::PackId::Python)
        );
        std::fs::write(dir.join("Makefile"), "all:\n").unwrap();
        assert_eq!(
            crate::pack::detect(&dir, "").unwrap(),
            crate::pack::Detected::Pack(crate::pack::PackId::Python)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn python_diff_scopes_findings_to_changed_py_files() {
        let dir = std::env::temp_dir().join(format!("sc-py-diff-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("tests")).unwrap();
        let key = format!("AKIA{}{}", "0Z3VS5J4", "AB3KQM9Z");
        std::fs::write(dir.join("pyproject.toml"), "[project]\nname = \"demo\"\n").unwrap();
        std::fs::write(
            dir.join("src/old.py"),
            format!("KEY = \"{key}\"\ndef broken(\n"),
        )
        .unwrap();
        std::fs::write(dir.join("src/new.py"), "def ok():\n    return 1\n").unwrap();
        std::fs::write(
            dir.join("tests/test_old.py"),
            "def test_old():\n    assert False\n",
        )
        .unwrap();
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
        std::fs::write(dir.join("src/new.py"), "def ok():\n    return 2\n").unwrap();
        let mut config = Config::default();
        config.engines.coverage = false;
        let request = |diff: Option<&str>| AnalyzeRequest {
            root: dir.clone(),
            repo: "demo".into(),
            fail_on: config.gates.fail_on.clone(),
            budget: Duration::from_secs(25),
            config: config.clone(),
            diff_base: diff.map(str::to_string),
            diff_head: None,
            path_list: Vec::new(),
            spec_path: None,
            mutation_override: None,
            llm_override: None,
            intent: None,
            progress: None,
        };
        let scoped = analyze(request(Some("HEAD")));
        assert_eq!(scoped.scorecard.context.scope.mode, "diff");
        assert_eq!(scoped.scorecard.context.scope.paths, vec!["src/new.py"]);
        assert_eq!(scoped.scorecard.context.scope.base.as_deref(), Some("HEAD"));
        assert_eq!(scoped.scorecard.context.scope.other_paths, Some(2));
        assert_eq!(scoped.scorecard.identity.test_selection, "diff-tests");
        assert_eq!(scoped.scorecard.measures.metrics.files_changed, 1);
        assert_eq!(scoped.scorecard.measures.metrics.loc_changed, 1);
        let types = scoped
            .scorecard
            .measures
            .gates
            .iter()
            .find(|gate| gate.id == "types")
            .unwrap();
        assert!(types.pass, "{:?}", scoped.scorecard.sections.findings);
        let tests = scoped
            .scorecard
            .measures
            .gates
            .iter()
            .find(|gate| gate.id == "tests")
            .unwrap();
        assert!(!tests.enforced, "{tests:?}");
        assert!(
            tests
                .reason
                .as_deref()
                .unwrap_or("")
                .contains("no Python tests"),
            "{tests:?}"
        );
        let secrets = scoped
            .scorecard
            .measures
            .gates
            .iter()
            .find(|gate| gate.id == "secrets")
            .unwrap();
        assert!(secrets.pass, "{:?}", scoped.scorecard.sections.findings);
        assert!(
            !scoped
                .scorecard
                .sections
                .findings
                .iter()
                .any(|finding| finding.file.contains("old.py")),
            "{:?}",
            scoped.scorecard.sections.findings
        );
        let lint = scoped
            .scorecard
            .runs
            .iter()
            .find(|run| run.engine == "lint")
            .expect("lint command");
        assert!(lint.command.contains("src/new.py"), "{}", lint.command);
        assert!(!lint.command.contains("check ."), "{}", lint.command);
        assert!(!lint.command.contains("old.py"), "{}", lint.command);

        let tree = analyze(request(None));
        assert_eq!(tree.scorecard.context.scope.mode, "tree");
        assert_eq!(tree.scorecard.identity.test_selection, "full-suite");
        let tree_types = tree
            .scorecard
            .measures
            .gates
            .iter()
            .find(|gate| gate.id == "types")
            .unwrap();
        assert!(!tree_types.pass, "tree compile must see src/old.py");
        assert!(
            tree.scorecard.sections.findings.iter().any(|finding| {
                finding.rule == "secrets.aws_access_key" && finding.file == "src/old.py"
            }),
            "{:?}",
            tree.scorecard.sections.findings
        );
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
        let clean = git_info(&dir, &[]);
        assert_eq!(clean.head.as_ref().unwrap().len(), 40);
        assert!(!clean.dirty);
        std::fs::write(dir.join("note.txt"), "x").unwrap();
        let dirty = git_info(&dir, &[]);
        assert!(dirty.dirty);
        assert_eq!(dirty.dirty_paths, ["note.txt"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_probe_ignores_generated_reports_and_names_other_changes() {
        let dir = git_repo("sc-git-report-outputs");
        let generated = [
            dir.join("scorecard.json"),
            dir.join("scorecard.md"),
            dir.join("scorecard.sarif"),
            dir.join("scorecard.html"),
        ];
        let last_report = dir.join(".sc/last-scorecard.json");
        let cache = dir.join(".sc/cache/parse-v2.json");
        let coverage = dir.join(".sc/coverage/rust.info");
        std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
        std::fs::create_dir_all(coverage.parent().unwrap()).unwrap();
        std::fs::write(&last_report, "generated by sc").unwrap();
        std::fs::write(&cache, "generated by sc").unwrap();
        std::fs::write(&coverage, "generated by sc").unwrap();
        for path in &generated {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "generated by sc").unwrap();
        }
        let clean = git_info(&dir, &generated);
        assert!(!clean.dirty, "generated reports were counted: {clean:?}");
        assert!(clean.dirty_paths.is_empty());

        std::fs::write(dir.join("note.txt"), "user change").unwrap();
        let dirty = git_info(&dir, &generated);
        assert!(dirty.dirty);
        assert_eq!(dirty.dirty_paths, ["note.txt"]);
        assert_eq!(dirty.status_label(), "dirty");
        assert_eq!(dirty.changed_paths_label(), "note.txt");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_probe_filters_report_outputs_from_earlier_runs() {
        let dir = git_repo("sc-git-own-outputs");
        std::fs::create_dir_all(dir.join(".sc")).unwrap();
        std::fs::write(dir.join(".sc").join("last-scorecard.json"), "{}").unwrap();
        let report = dir.join("scorecard.html");
        std::fs::write(&report, "x").unwrap();
        assert!(
            !git_info(&dir, &[report]).dirty,
            "earlier reports should not make a clean tree dirty"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn progress_callback_sees_steps_without_changing_the_scorecard() {
        use std::sync::{Arc, Mutex};
        // A clean repo with no detectable pack takes the fast blocked path.
        let dir = git_repo("sc-git-progress");
        let base = AnalyzeRequest {
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
            progress: None,
        };
        let plain = analyze(base.clone());
        let steps: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&steps);
        let mut with_progress = base.clone();
        with_progress.progress = Some(Arc::new(move |step: &str| {
            seen.lock().unwrap().push(step.to_string());
        }));
        let reported = analyze(with_progress);
        let steps = steps.lock().unwrap();
        assert!(
            steps.first().is_some_and(|step| step == "Detecting pack"),
            "first step should name pack detection, got {steps:?}"
        );
        assert!(
            steps.last().is_some_and(|step| step == "Writing report"),
            "last step should name the report, got {steps:?}"
        );
        let mut plain_json = serde_json::to_value(&plain.scorecard).unwrap();
        let mut reported_json = serde_json::to_value(&reported.scorecard).unwrap();
        // Scorecard ids are unique per run; everything else must match.
        plain_json["id"] = serde_json::Value::Null;
        reported_json["id"] = serde_json::Value::Null;
        assert_eq!(plain_json, reported_json);
        assert_eq!(plain.status, reported.status);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn blocked_analysis_reports_pre_analysis_git_state() {
        // analyze() snapshots git before any engine runs: a clean repo with
        // no detectable pack reports clean with the committed head.
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
            progress: None,
        });
        assert!(!output.scorecard.context.git.dirty);
        assert_eq!(
            output.scorecard.context.git.head.as_ref().unwrap().len(),
            40
        );
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
            other_paths: None,
            workspace_root: true,
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
            other_paths: None,
            workspace_root: true,
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
    fn a_dependency_of_another_crate_is_still_undeclared() {
        use crate::scope::Selection;
        let dir = std::env::temp_dir().join(format!("sc-sca-sibling-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a/src")).unwrap();
        std::fs::create_dir_all(dir.join("b/src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers = [\"a\", \"b\"]\n\n[workspace.dependencies]\nserde = \"1\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("a/Cargo.toml"),
            "[package]\nname = \"crate-a\"\nversion = \"0.1.0\"\n\n[dependencies]\nhtml5ever = \"0.1\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("b/Cargo.toml"),
            "[package]\nname = \"crate-b\"\nversion = \"0.1.0\"\n\n[dependencies]\nbytes = \"1\"\n",
        )
        .unwrap();
        let files = vec![
            analyzed("a/src/lib.rs", &["html5ever", "serde"], &[]),
            analyzed("b/src/lib.rs", &["html5ever", "bytes", "serde"], &[]),
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
            other_paths: None,
            workspace_root: true,
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
        assert_eq!(findings[0].file, "b/src/lib.rs");
        assert_eq!(findings[0].symbol.as_deref(), Some("html5ever"));
        assert!(findings[0].message.contains("this crate's Cargo.toml"));
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
    fn one_unscored_function_does_not_hide_a_measured_crap_failure() {
        let gate = crap_gate(false, 1, 0, 15);
        assert!(!gate.pass);
        assert!(gate.enforced);
        let reason = gate.reason.unwrap();
        assert!(reason.contains("1 function over threshold"), "{reason}");
        assert!(reason.contains("no coverage record"), "{reason}");

        let untested = crap_gate(false, 0, 1, 15);
        assert!(!untested.pass);
        assert!(untested.enforced);

        let clear = crap_gate(false, 0, 0, 15);
        assert!(!clear.pass);
        assert!(!clear.enforced);
        assert_eq!(
            clear.reason.as_deref(),
            Some("some functions have no coverage record and were not scored")
        );
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

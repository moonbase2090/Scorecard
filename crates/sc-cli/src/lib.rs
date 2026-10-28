// SPDX-License-Identifier: MIT
//! CLI for the `sc` scorecard gate.
//!
//! User-facing analyze output is the scorecard, rendered as JSON and/or Markdown.

mod console;
mod doctor;
mod format;
mod pretty;
mod progress;
mod report;
mod setup;
mod skills;
#[cfg(feature = "tui")]
mod tui;
mod user_config;

use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use sc_core::{
    apply_disposition, compute_scores, load_config_file, normalize_gates, resolve_config_path,
    Finding, Gate, Scorecard, SCORECARD_VERSION,
};
use sc_core::{gates_failing_on_new_findings, Baseline, BASELINE_VERSION};
use sc_engines::{
    analyze_with_generated_paths, os_metadata_warning, AnalyzeOutput, AnalyzeRequest, RunStatus,
};

pub use format::{to_json, to_markdown};
use report::to_html;

#[derive(Parser)]
#[command(
    name = "sc",
    version,
    about = "Local-first deterministic code quality gate"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Args)]
struct AnalyzeArgs {
    /// Project directory. Defaults to `.`.
    path: Option<PathBuf>,
    /// `json`, `pretty`, `md`, `sarif`, `html`, or `all`.
    /// Omitted: `pretty` when stdout is a terminal, otherwise `json`.
    #[arg(long, value_parser = ["json", "pretty", "md", "sarif", "html", "all"])]
    format: Option<String>,
    /// Also write the report to this path. Stdout prints a short summary.
    /// Without this flag, stdout is the full report.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Gates that set exit 1 when they fail. Use `none` by itself to suppress every gate failure.
    /// An empty list is invalid. The report verdict still reflects every enforced gate.
    /// Default: types,tests,crap,secrets,lint.
    #[arg(long, value_name = "LIST")]
    fail_on: Option<String>,
    /// Write every finding's fingerprint to `.sc/baseline.json` in the project.
    /// Run once on a clean or accepted state, then gate with `--baseline`.
    #[arg(long, conflicts_with = "baseline")]
    write_baseline: bool,
    /// Exit 1 only when a failing gate fails on a finding not in `.sc/baseline.json`.
    /// Known findings still appear in the report. Exits 2 when there is no baseline.
    #[arg(long)]
    baseline: bool,
    /// Pack override when several manifests match, or `multi` to analyze pack roots in a monorepo.
    #[arg(long, value_name = "PACK")]
    pack: Option<String>,
    /// Score only the git diff against BASE. Omit BASE for AUTO (HEAD~1, else
    /// main, else master). A candidate that is HEAD itself is skipped. AUTO
    /// and a missing explicit BASE exit 2 with verdict fail.
    #[arg(long, num_args = 0..=1, default_missing_value = "AUTO")]
    diff: Option<String>,
    /// Compare `--diff` BASE to this commit instead of the worktree.
    #[arg(long)]
    diff_head: Option<String>,
    /// Newline-separated source paths. Mutually exclusive with `--diff`.
    #[arg(long)]
    paths: Option<PathBuf>,
    /// Spec or task file. Public items and named files must exist.
    #[arg(long)]
    spec: Option<PathBuf>,
    /// `off` (default), `diff`, or `full`.
    #[arg(long, value_parser = ["off", "diff", "full"])]
    mutation: Option<String>,
    /// `off` (default) or `on`. On runs an OpenAI-compatible spec review.
    #[arg(long, value_parser = ["off", "on"])]
    llm: Option<String>,
    /// What this change is supposed to accomplish. Stored on the scorecard.
    #[arg(long)]
    intent: Option<String>,
    /// Wall-clock budget for pack commands, in seconds.
    #[arg(long, default_value_t = 120)]
    budget_seconds: u64,
    /// Path to analyzer.toml. Default: ./analyzer.toml, then ~/.config/sc/analyzer.toml.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Suppress the progress spinner on stderr.
    #[arg(long)]
    quiet: bool,
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// Print ~/.config/sc/analyzer.toml.
    Path,
    /// Write a starter analyzer.toml to ~/.config/sc. Does not replace an existing file.
    Init {
        /// Replace an existing file.
        #[arg(long)]
        force: bool,
    },
}

#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
enum Commands {
    /// Browse a saved scorecard in an interactive terminal.
    #[cfg(feature = "tui")]
    Tui {
        /// Project directory containing .sc/last-scorecard.json. Defaults to `.`.
        path: Option<PathBuf>,
        /// Print a compact saved summary without entering the alternate screen.
        #[arg(long)]
        inline: bool,
    },
    /// Analyze a project tree and print a scorecard.
    Analyze(AnalyzeArgs),
    /// List the tools the project's language pack needs and how to install the missing ones.
    Doctor {
        /// Project directory. Defaults to `.`.
        path: Option<PathBuf>,
        /// Pack to check when several manifests match: rust, node, python, bash, go, java, csharp, php, cpp, or web.
        #[arg(long, value_name = "PACK")]
        pack: Option<String>,
    },
    /// Print or create ~/.config/sc/analyzer.toml.
    Config {
        #[command(subcommand)]
        command: ConfigCmd,
    },
    /// Install the agent skill and register the sc-mcp server for this user.
    Setup,
    /// Install the Scorecard skill for coding agents.
    Skills {
        #[command(subcommand)]
        command: skills::SkillsCommand,
    },
}

struct View<'a> {
    stdout_format: &'a str,
    file_format: &'a str,
    color: bool,
    width: usize,
    tty: bool,
    /// True when `--baseline` decides the exit code, so enforced failures
    /// that the baseline covers are not reported as unexpected.
    baseline: bool,
}

/// Pick what lands on stdout. With `--out`, print the short summary in
/// every environment and write the chosen format to the file. Without
/// `--out`, stdout is the chosen format, so `sc analyze --format json | jq .`
/// is unchanged.
fn stdout_body_format(stdout_format: &str, has_out: bool, _tty: bool) -> &str {
    if has_out {
        "pretty"
    } else {
        stdout_format
    }
}

pub fn run() -> i32 {
    // Enable VT sequences before any pretty / spinner output. Pipes and
    // rejected consoles return false; color then stays plain unless forced.
    let _vt = console::enable_console_vt();
    let cli = Cli::parse();
    match cli.command {
        Commands::Doctor { path, pack } => doctor::run(path, pack),
        #[cfg(feature = "tui")]
        Commands::Tui { path, inline } => {
            tui::run(path.as_deref().unwrap_or_else(|| Path::new(".")), inline)
        }
        Commands::Setup => setup::run(),
        Commands::Skills { command } => skills::run(command),
        Commands::Config { command } => match command {
            ConfigCmd::Path => user_config::run_path(),
            ConfigCmd::Init { force } => user_config::run_init(force),
        },
        Commands::Analyze(args) => analyze_cmd(args, _vt),
    }
}

fn analyze_cmd(args: AnalyzeArgs, vt_ready: bool) -> i32 {
    let AnalyzeArgs {
        path,
        format,
        out,
        fail_on,
        pack,
        diff,
        diff_head,
        paths,
        spec,
        mutation,
        llm,
        intent,
        budget_seconds,
        config,
        quiet,
        write_baseline,
        baseline,
    } = args;
    let tty = io::IsTerminal::is_terminal(&io::stdout());
    let stdout_format = pretty::stdout_format(format.as_deref(), tty);
    let file_format = format.unwrap_or_else(|| "json".to_string());
    // Color needs a TTY that accepted VT (Unix always accepts). CLICOLOR_FORCE
    // still wins inside use_color when set to something other than "0".
    let color_tty = tty && vt_ready;
    let view = View {
        stdout_format,
        file_format: &file_format,
        color: pretty::use_color(
            color_tty,
            std::env::var_os("NO_COLOR").is_some(),
            std::env::var("CLICOLOR_FORCE").ok().as_deref(),
            std::env::var("CLICOLOR").ok().as_deref(),
        ),
        width: pretty::term_width(),
        tty,
        baseline,
    };
    let path = path.unwrap_or_else(|| PathBuf::from("."));
    let repo = display_repo(&path);
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    if !path.exists() {
        let card = early_card(
            &repo,
            "engine.unavailable",
            "compile",
            &format!("path does not exist: {}", path.display()),
        );
        return emit(
            &mut stdout,
            &view,
            out.as_deref(),
            &card,
            RunStatus::AnalyzerError,
        );
    }

    let root = crate_root(&path);
    let root = match fs::canonicalize(&root) {
        Ok(root) => root,
        Err(err) => {
            let card = early_card(
                &repo,
                "engine.unavailable",
                "compile",
                &format!("cannot read {}: {err}", path.display()),
            );
            return emit(
                &mut stdout,
                &view,
                out.as_deref(),
                &card,
                RunStatus::AnalyzerError,
            );
        }
    };

    let baseline_file = root.join(".sc").join("baseline.json");
    let known = if baseline {
        match read_baseline(&baseline_file) {
            Ok(known) => Some(known),
            Err(err) => {
                let _ = writeln!(io::stderr(), "sc: {err}");
                let card = early_card(&repo, "baseline.invalid", "baseline", &err);
                return emit(
                    &mut stdout,
                    &view,
                    out.as_deref(),
                    &card,
                    RunStatus::AnalyzerError,
                );
            }
        }
    } else {
        None
    };

    let config_path = resolve_config_path(config.as_deref(), &root);
    let mut loaded = match load_config_file(config_path.as_deref()) {
        Ok(config) => config,
        Err(err) => {
            let _ = writeln!(io::stderr(), "sc: {err}");
            let card = early_card(&repo, "config.invalid", "config", &err);
            return emit(
                &mut stdout,
                &view,
                out.as_deref(),
                &card,
                RunStatus::AnalyzerError,
            );
        }
    };

    if let Some(list) = fail_on {
        match normalize_gates(&[list]) {
            Ok(gates) => loaded.gates.fail_on = gates,
            Err(err) => {
                let _ = writeln!(io::stderr(), "sc: {err}");
                let card = early_card(&repo, "config.invalid", "config", &err);
                return emit(
                    &mut stdout,
                    &view,
                    out.as_deref(),
                    &card,
                    RunStatus::AnalyzerError,
                );
            }
        }
    }

    if let Some(pack) = pack {
        loaded.pack = pack;
    }
    let fail_on = loaded.gates.fail_on.clone();
    let path_list = match paths {
        Some(path) => match fs::read_to_string(&path) {
            Ok(text) => text
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .map(str::to_string)
                .collect(),
            Err(err) => {
                let card = early_card(
                    &repo,
                    "config.invalid",
                    "config",
                    &format!("cannot read paths file: {err}"),
                );
                return emit(
                    &mut stdout,
                    &view,
                    out.as_deref(),
                    &card,
                    RunStatus::AnalyzerError,
                );
            }
        },
        None => Vec::new(),
    };
    if let Some(warning) = os_metadata_warning(
        &root,
        &loaded.scope.exclude,
        &loaded.scope.include_generated,
    ) {
        let _ = writeln!(io::stderr(), "{warning}");
    }
    let generated_paths = generated_output_paths(&root, out.as_deref(), &file_format);
    // The spinner animates on stderr only; stdout stays byte-identical.
    // It starts just before the long analysis run and is cleared before
    // the report prints.
    let stderr_tty = io::IsTerminal::is_terminal(&io::stderr());
    let spinner = if progress::should_show_from_env(stderr_tty, quiet) {
        Some(progress::Spinner::start("Detecting pack"))
    } else {
        None
    };
    let mut output = analyze_with_generated_paths(
        AnalyzeRequest {
            root,
            repo,
            fail_on,
            budget: Duration::from_secs(budget_seconds),
            config: loaded,
            diff_base: diff,
            diff_head,
            path_list,
            spec_path: spec,
            mutation_override: mutation,
            llm_override: llm.map(|value| value == "on"),
            intent,
            progress: spinner.as_ref().map(progress::Spinner::updater),
        },
        &generated_paths,
    );
    if let Some(spinner) = spinner {
        spinner.finish();
    }
    finish_baseline(&mut output, write_baseline, known.as_ref(), &baseline_file);
    emit(
        &mut stdout,
        &view,
        out.as_deref(),
        &output.scorecard,
        output.status,
    )
}

fn emit(
    stdout: &mut impl Write,
    view: &View<'_>,
    out: Option<&Path>,
    card: &Scorecard,
    status: RunStatus,
) -> i32 {
    let json = to_json(card);
    let md = to_markdown(card);
    let sarif = sc_sarif::to_sarif(card);
    let html = to_html(card);
    let mut code = match status {
        RunStatus::Passed => 0,
        RunStatus::GateFailed => 1,
        RunStatus::AnalyzerError => 2,
    };
    let pretty = pretty::to_pretty(
        card,
        &pretty::PrettyOpts {
            color: view.color,
            width: view.width,
            version: env!("CARGO_PKG_VERSION").to_string(),
            report: out.map(|path| path.display().to_string()),
            exit_code: code,
        },
    );
    if let Some(path) = out {
        if let Err(err) = write_reports(view.file_format, path, &json, &md, &sarif, &html, &pretty)
        {
            let _ = writeln!(io::stderr(), "sc: failed to write report: {err}");
            code = 2;
        }
    }
    if code == 0 {
        let failed_enforced: Vec<_> = card
            .measures
            .gates
            .iter()
            .filter(|gate| gate.enforced && !gate.pass)
            .map(|gate| gate.id.as_str())
            .collect();
        if !failed_enforced.is_empty() && !view.baseline {
            let _ = writeln!(
                io::stderr(),
                "sc: exit 0 although enforced gates failed: {} (not in --fail-on)",
                failed_enforced.join(", ")
            );
        }
    }
    note_unresolved_diff_base(card, status);
    let body = match stdout_body_format(view.stdout_format, out.is_some(), view.tty) {
        "md" => md,
        "sarif" => sarif,
        // HTML to a terminal is noise; `all` keeps JSON+Markdown on stdout
        // and still writes the .html sibling when --out is set.
        "html" => html,
        "all" => format!("{json}\n{md}"),
        "pretty" => pretty,
        _ => json,
    };
    let _ = writeln!(stdout, "{body}");
    code
}

fn note_unresolved_diff_base(card: &Scorecard, status: RunStatus) {
    if status != RunStatus::AnalyzerError {
        return;
    }
    let Some(finding) = card.sections.findings.iter().find(|finding| {
        finding.rule == "engine.unavailable"
            && (finding.message.contains("fetch-depth")
                || finding.message.contains("ref not found"))
    }) else {
        return;
    };
    let line = finding.message.replace(['\n', '\r'], " ");
    let _ = writeln!(io::stderr(), "{line}");
}

/// Write or apply the baseline once the analysis has run. Writing never changes
/// the outcome beyond an error; `known` can turn a gate failure into exit 0.
fn finish_baseline(output: &mut AnalyzeOutput, write: bool, known: Option<&Baseline>, file: &Path) {
    if write {
        match write_baseline_file(file, &output.scorecard.sections.findings) {
            Ok(count) => {
                let _ = writeln!(
                    io::stderr(),
                    "sc: wrote {count} finding fingerprints to {}",
                    file.display()
                );
            }
            Err(err) => {
                let _ = writeln!(io::stderr(), "sc: {err}");
                output.status = RunStatus::AnalyzerError;
            }
        }
    }
    if let Some(known) = known {
        // Only gates that --fail-on selects can block; a new finding in an
        // unselected gate is reported but does not change the exit code.
        let blocking = gates_failing_on_new_findings(&output.scorecard, known)
            .iter()
            .any(|id| output.scorecard.fail_on.contains(id));
        if output.status == RunStatus::GateFailed && !blocking {
            output.status = RunStatus::Passed;
            let _ = writeln!(
                io::stderr(),
                "sc: exit 0: every failing --fail-on gate fails only on findings in {}",
                file.display()
            );
        }
    }
}

fn read_baseline(path: &Path) -> Result<Baseline, String> {
    let text = fs::read_to_string(path).map_err(|_| {
        format!(
            "no baseline at {}. Run `sc analyze --write-baseline` first.",
            path.display()
        )
    })?;
    let baseline: Baseline = serde_json::from_str(&text)
        .map_err(|err| format!("cannot parse {}: {err}", path.display()))?;
    if baseline.version != BASELINE_VERSION {
        return Err(format!(
            "{} is baseline version {}, expected {BASELINE_VERSION}. Run `sc analyze --write-baseline` again.",
            path.display(),
            baseline.version
        ));
    }
    Ok(baseline)
}

fn write_baseline_file(path: &Path, findings: &[Finding]) -> Result<usize, String> {
    let baseline = Baseline::from_findings(findings);
    let text = serde_json::to_string_pretty(&baseline).map_err(|err| err.to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("{}: {err}", parent.display()))?;
    }
    fs::write(path, format!("{text}\n")).map_err(|err| format!("{}: {err}", path.display()))?;
    Ok(baseline.fingerprints.len())
}

fn write_reports(
    format: &str,
    path: &Path,
    json: &str,
    md: &str,
    sarif: &str,
    html: &str,
    pretty: &str,
) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    match format {
        "md" => fs::write(path, format!("{md}\n")),
        "sarif" => fs::write(path, format!("{sarif}\n")),
        "html" => fs::write(path, format!("{html}\n")),
        "pretty" => fs::write(path, format!("{pretty}\n")),
        "all" => {
            fs::write(json_target(path), format!("{json}\n"))?;
            fs::write(md_target(path), format!("{md}\n"))?;
            fs::write(sarif_target(path), format!("{sarif}\n"))?;
            fs::write(html_target(path), format!("{html}\n"))
        }
        _ => fs::write(path, format!("{json}\n")),
    }
}

fn sarif_target(path: &Path) -> PathBuf {
    path.with_extension("sarif")
}

fn html_target(path: &Path) -> PathBuf {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("html") => path.to_path_buf(),
        _ => path.with_extension("html"),
    }
}

fn json_target(path: &Path) -> PathBuf {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("json") => path.to_path_buf(),
        Some("md") => path.with_extension("json"),
        _ => path.with_extension("json"),
    }
}

fn generated_output_paths(root: &Path, out: Option<&Path>, format: &str) -> Vec<PathBuf> {
    let mut paths = vec![root.join(".sc/last-scorecard.json")];
    let Some(out) = out else {
        return paths;
    };
    let report_paths = if format == "all" {
        vec![
            json_target(out),
            md_target(out),
            sarif_target(out),
            html_target(out),
        ]
    } else {
        vec![out.to_path_buf()]
    };
    let Ok(current_dir) = std::env::current_dir() else {
        return paths;
    };
    let current_dir = fs::canonicalize(&current_dir).unwrap_or(current_dir);
    for path in report_paths {
        let absolute = if path.is_absolute() {
            path
        } else {
            current_dir.join(path)
        };
        let absolute = normalize_path(&absolute);
        let normalized = absolute
            .parent()
            .and_then(|parent| fs::canonicalize(parent).ok())
            .and_then(|parent| absolute.file_name().map(|name| parent.join(name)))
            .unwrap_or(absolute);
        paths.push(normalized);
    }
    paths
}

fn normalize_path(path: &Path) -> PathBuf {
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

fn md_target(path: &Path) -> PathBuf {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("md") => path.to_path_buf(),
        _ => path.with_extension("md"),
    }
}

fn crate_root(path: &Path) -> PathBuf {
    if path.is_file() && path.file_name().and_then(|name| name.to_str()) == Some("Cargo.toml") {
        path.parent().unwrap_or(path).to_path_buf()
    } else {
        path.to_path_buf()
    }
}

fn display_repo(path: &Path) -> String {
    if path == Path::new(".") {
        return ".".to_string();
    }
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_string()
}

fn early_card(repo: &str, rule: &str, engine: &str, message: &str) -> Scorecard {
    let mut card = Scorecard::skeleton(repo, 30);
    card.identity.version = SCORECARD_VERSION.to_string();
    let severity = if rule == "engine.unavailable" {
        "warning"
    } else {
        "error"
    };
    let id = if rule == "engine.unavailable" {
        format!("{engine}:unavailable")
    } else {
        format!("{engine}:{rule}")
    };
    card.sections.findings.push(Finding {
        id,
        rule: rule.to_string(),
        engine: engine.to_string(),
        severity: severity.to_string(),
        file: ".".into(),
        span: None,
        symbol: None,
        message: message.to_string(),
        evidence: serde_json::json!({}),
        suggested_action: Some("Fix the analyzer invocation and re-run".into()),
        disposition: String::new(),
    });
    card.measures.gates = vec![
        Gate {
            id: "types".into(),
            pass: false,
            enforced: true,
            reason: Some(message.to_string()),
        },
        Gate {
            id: "tests".into(),
            pass: false,
            enforced: true,
            reason: Some("not run".into()),
        },
        Gate {
            id: "crap".into(),
            pass: false,
            enforced: true,
            reason: Some("not run".into()),
        },
    ];
    card.measures.scores = compute_scores(&card.sections.findings);
    card.verdict = "fail".into();
    apply_disposition(&mut card.sections.findings);
    card
}

#[cfg(test)]
mod skill_command_docs_test {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn every_top_level_command_is_named_in_the_embedded_skill() {
        let skill = include_str!("../../../skills/scorecard/SKILL.md");
        for command in Cli::command().get_subcommands() {
            let name = command.get_name();
            assert!(
                skill.contains(&format!("`sc {name}")),
                "top-level command `sc {name}` is missing from skills/scorecard/SKILL.md"
            );
        }
    }
}

#[cfg(test)]
mod stdout_body_format_tests {
    use super::*;

    #[test]
    fn out_on_a_terminal_prints_the_summary_otherwise_the_full_report() {
        // (stdout_format, has_out, tty) -> body format
        let cases = [
            ("html", true, true, "pretty"),
            ("md", true, true, "pretty"),
            ("sarif", true, true, "pretty"),
            ("json", true, true, "pretty"),
            ("all", true, true, "pretty"),
            ("pretty", true, true, "pretty"),
            ("html", true, false, "pretty"),
            ("md", true, false, "pretty"),
            ("sarif", true, false, "pretty"),
            ("json", true, false, "pretty"),
            ("all", true, false, "pretty"),
            ("html", false, true, "html"),
            ("json", false, true, "json"),
            ("json", false, false, "json"),
            ("pretty", false, true, "pretty"),
        ];
        for (format, has_out, tty, expected) in cases {
            assert_eq!(
                stdout_body_format(format, has_out, tty),
                expected,
                "format={format} has_out={has_out} tty={tty}"
            );
        }
    }
}

#[cfg(test)]
mod baseline_gate_tests {
    use super::*;
    use sc_core::Gate;
    use serde_json::json;

    fn finding(engine: &str, message: &str) -> Finding {
        Finding {
            id: format!("{engine}-{message}"),
            rule: format!("{engine}.rule"),
            engine: engine.into(),
            severity: "error".into(),
            file: "src/lib.rs".into(),
            span: None,
            symbol: None,
            message: message.into(),
            evidence: json!({}),
            suggested_action: None,
            disposition: String::new(),
        }
    }

    /// `tests` (selected by --fail-on) and `lint` (not selected) both fail.
    /// The known finding is in the baseline; `new_engine` gets a new one.
    fn outcome(new_engine: &str) -> AnalyzeOutput {
        let known = finding("tests", "known failure");
        let mut scorecard = Scorecard::skeleton("repo", 30);
        scorecard.fail_on = vec!["tests".into()];
        scorecard.measures.gates = ["tests", "lint"]
            .iter()
            .map(|id| Gate {
                id: (*id).into(),
                pass: false,
                enforced: true,
                reason: None,
            })
            .collect();
        scorecard.sections.findings = vec![known.clone(), finding(new_engine, "new failure")];
        AnalyzeOutput {
            scorecard,
            status: RunStatus::GateFailed,
        }
    }

    fn baseline() -> Baseline {
        Baseline::from_findings(&[finding("tests", "known failure")])
    }

    #[test]
    fn a_new_finding_outside_fail_on_does_not_block() {
        let mut out = outcome("lint");
        finish_baseline(
            &mut out,
            false,
            Some(&baseline()),
            Path::new(".sc/baseline.json"),
        );
        assert!(
            matches!(out.status, RunStatus::Passed),
            "lint must not block"
        );
    }

    #[test]
    fn a_new_finding_in_a_selected_gate_blocks() {
        let mut out = outcome("tests");
        finish_baseline(
            &mut out,
            false,
            Some(&baseline()),
            Path::new(".sc/baseline.json"),
        );
        assert!(
            matches!(out.status, RunStatus::GateFailed),
            "tests must block"
        );
    }
}

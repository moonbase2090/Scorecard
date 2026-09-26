// SPDX-License-Identifier: MPL-2.0
//! CLI for the `sc` scorecard gate.
//!
//! User-facing analyze output is the scorecard, rendered as JSON and/or Markdown.

mod format;
mod report;
mod setup;

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use sc_core::{
    apply_disposition, compute_scores, load_config_file, normalize_gates, resolve_config_path,
    Finding, Gate, Scorecard, SCORECARD_VERSION,
};
use sc_engines::{analyze, AnalyzeRequest, RunStatus};

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
    /// `json` (default), `md`, `sarif`, `html`, or `all`.
    #[arg(long, value_parser = ["json", "md", "sarif", "html", "all"], default_value = "json")]
    format: String,
    /// Also write the report to this path.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Comma-separated gates that fail the run. Default: types,tests,crap,secrets,lint.
    #[arg(long, value_name = "LIST")]
    fail_on: Option<String>,
    /// Pack override when several manifests match: rust, node, python, bash, go, java, csharp, php, cpp, or command.
    #[arg(long, value_name = "PACK")]
    pack: Option<String>,
    /// Score only the git diff against BASE. Omit BASE to use HEAD~1, else main.
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
}

#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
enum Commands {
    /// Analyze a project tree and print a scorecard.
    Analyze(AnalyzeArgs),
    /// Install the agent skill and register the sc-mcp server for this user.
    Setup,
}

pub fn run() -> i32 {
    let cli = Cli::parse();
    match cli.command {
        Commands::Setup => setup::run(),
        Commands::Analyze(args) => analyze_cmd(args),
    }
}

fn analyze_cmd(args: AnalyzeArgs) -> i32 {
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
    } = args;
    let format = format.as_str();
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
            format,
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
                format,
                out.as_deref(),
                &card,
                RunStatus::AnalyzerError,
            );
        }
    };

    let config_path = resolve_config_path(config.as_deref(), &root);
    let mut loaded = match load_config_file(config_path.as_deref()) {
        Ok(config) => config,
        Err(err) => {
            let card = early_card(&repo, "config.invalid", "config", &err);
            return emit(
                &mut stdout,
                format,
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
                let card = early_card(&repo, "config.invalid", "config", &err);
                return emit(
                    &mut stdout,
                    format,
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
                    format,
                    out.as_deref(),
                    &card,
                    RunStatus::AnalyzerError,
                );
            }
        },
        None => Vec::new(),
    };
    let output = analyze(AnalyzeRequest {
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
    });
    emit(
        &mut stdout,
        format,
        out.as_deref(),
        &output.scorecard,
        output.status,
    )
}

fn emit(
    stdout: &mut impl Write,
    format: &str,
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
    if let Some(path) = out {
        if let Err(err) = write_reports(format, path, &json, &md, &sarif, &html) {
            let _ = writeln!(io::stderr(), "sc: failed to write report: {err}");
            code = 2;
        }
    }
    let body = match format {
        "md" => md,
        "sarif" => sarif,
        // HTML to a terminal is noise; `all` keeps JSON+Markdown on stdout
        // and still writes the .html sibling when --out is set.
        "html" => html,
        "all" => format!("{json}\n{md}"),
        _ => json,
    };
    let _ = writeln!(stdout, "{body}");
    code
}

fn write_reports(
    format: &str,
    path: &Path,
    json: &str,
    md: &str,
    sarif: &str,
    html: &str,
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
    card.version = SCORECARD_VERSION.to_string();
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
    card.findings.push(Finding {
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
    card.gates = vec![
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
    card.scores = compute_scores(&card.findings);
    card.verdict = "fail".into();
    apply_disposition(&mut card.findings);
    card
}

// SPDX-License-Identifier: MPL-2.0
//! Human-readable scorecard for a terminal.
//!
//! Color follows `NO_COLOR`, `CLICOLOR_FORCE`, and whether stdout is a TTY.
//! The plain style uses ASCII so a captured transcript stays readable.

use crate::report::Outcome;
use anstyle::{AnsiColor, Color, Style};
use sc_core::{CrapFunction, Finding, Gate, Scorecard};

const FINDING_LIMIT: usize = 8;
const MIN_WIDTH: usize = 40;

pub struct PrettyOpts {
    pub color: bool,
    pub width: usize,
    pub version: String,
    pub report: Option<String>,
    pub exit_code: i32,
}

pub fn stdout_format(explicit: Option<&str>, tty: bool) -> &'static str {
    match explicit {
        Some("md") => "md",
        Some("sarif") => "sarif",
        Some("html") => "html",
        Some("all") => "all",
        Some("pretty") => "pretty",
        Some(_) => "json",
        None if tty => "pretty",
        None => "json",
    }
}

/// `NO_COLOR` wins. `CLICOLOR_FORCE` other than `0` forces color.
/// `CLICOLOR=0` disables it. Otherwise color follows the TTY.
pub fn use_color(
    tty: bool,
    no_color: bool,
    clicolor_force: Option<&str>,
    clicolor: Option<&str>,
) -> bool {
    if no_color {
        return false;
    }
    if let Some(force) = clicolor_force {
        return force != "0";
    }
    if clicolor == Some("0") {
        return false;
    }
    tty
}

pub fn term_width() -> usize {
    if let Ok(raw) = std::env::var("COLUMNS") {
        if let Ok(cols) = raw.parse::<usize>() {
            if cols >= MIN_WIDTH {
                return cols;
            }
        }
    }
    unix_cols().unwrap_or(80)
}

pub fn to_pretty(card: &Scorecard, opts: &PrettyOpts) -> String {
    let width = opts.width.max(MIN_WIDTH);
    let mut out = String::new();
    push_header(&mut out, card, opts, width);
    push_banner(&mut out, card, opts);
    push_gates(&mut out, card, opts, width);
    push_scores(&mut out, card, opts, width);
    push_crap(&mut out, card, opts, width);
    push_findings(&mut out, card, opts, width);
    push_llm(&mut out, card, opts, width);
    push_footer(&mut out, card, opts);
    out
}

fn push_header(out: &mut String, card: &Scorecard, opts: &PrettyOpts, width: usize) {
    let state = if card.git.dirty { "dirty" } else { "clean" };
    let line = format!(
        "sc {}  {}  {}  {} {}  scope {}",
        opts.version,
        card.repo,
        pack_name(card),
        short_sha(&card.git.head),
        state,
        card.scope.mode
    );
    out.push_str(&fit(&line, width, opts.color));
    out.push('\n');
    out.push('\n');
}

fn push_banner(out: &mut String, card: &Scorecard, opts: &PrettyOpts) {
    let outcome = Outcome::of(card);
    let (word, style) = match outcome {
        Outcome::Fail => ("FAIL", red()),
        Outcome::ReportOnly(_) => ("REPORT ONLY", blue()),
        Outcome::Pass | Outcome::Advisory(_) => ("PASS", green()),
    };
    out.push_str(&paint(opts.color, style.bold(), word));
    if let Some(note) = outcome.note() {
        out.push_str("  ");
        out.push_str(&note);
    }
    out.push('\n');
    out.push('\n');
}

fn push_gates(out: &mut String, card: &Scorecard, opts: &PrettyOpts, width: usize) {
    out.push_str("gates\n");
    if card.gates.is_empty() {
        out.push_str("  (none)\n\n");
        return;
    }
    for gate in &card.gates {
        out.push_str(&gate_row(gate, opts, width));
        out.push('\n');
    }
    out.push('\n');
}

fn gate_row(gate: &Gate, opts: &PrettyOpts, width: usize) -> String {
    let symbol = mark(gate.pass, opts.color);
    let style = if gate.pass { green() } else { red() };
    let painted = paint(opts.color, style, symbol);
    let kind = if gate.enforced {
        "enforced"
    } else {
        "advisory"
    };
    let mut line = format!("  {painted}  {:<16} {kind}", gate.id);
    if !gate.pass {
        if let Some(reason) = gate.reason.as_deref().filter(|reason| !reason.is_empty()) {
            let used = 2 + symbol.chars().count() + 2 + 16 + 1 + kind.len() + 2;
            line.push_str("  ");
            line.push_str(&fit(reason, width.saturating_sub(used), opts.color));
        }
    }
    line
}

fn push_scores(out: &mut String, card: &Scorecard, opts: &PrettyOpts, width: usize) {
    out.push_str("scores\n");
    score_line(out, "correctness", card.scores.correctness, opts, width);
    score_line(out, "efficiency", card.scores.efficiency, opts, width);
    score_line(
        out,
        "maintainability",
        card.scores.maintainability,
        opts,
        width,
    );
    score_line(out, "security", card.scores.security, opts, width);
    score_line(out, "a11y", card.scores.a11y, opts, width);
    out.push('\n');
}

fn score_line(out: &mut String, name: &str, score: f64, opts: &PrettyOpts, width: usize) {
    let text = format!("  {name:<16} {score:>4.2}  {}", bar(score));
    let text = fit(&text, width, opts.color);
    out.push_str(&paint(opts.color, dim(), &text));
    out.push('\n');
}

fn push_crap(out: &mut String, card: &Scorecard, opts: &PrettyOpts, width: usize) {
    out.push_str(&format!("worst crap  threshold {}\n", card.crap.threshold));
    if let Some(line) = crate::report::diff_baseline_short(card) {
        let line = format!("  {line}");
        out.push_str(&paint(opts.color, dim(), &fit(&line, width, opts.color)));
        out.push('\n');
    }
    let rows: Vec<&CrapFunction> = card.crap.worst.iter().take(5).collect();
    if rows.is_empty() {
        out.push_str("  (none)\n\n");
        return;
    }
    let measured = crate::report::coverage_measured(card);
    if !measured {
        let note = "  coverage not measured: CRAP assumes 0% coverage (upper bound)";
        out.push_str(&paint(opts.color, dim(), &fit(note, width, opts.color)));
        out.push('\n');
    }
    out.push_str("  CRAP   CC   COV  SYMBOL            LOCATION\n");
    for row in rows {
        let cov = if measured {
            format!("{}%", (row.coverage.clamp(0.0, 1.0) * 100.0).round() as i64)
        } else {
            "--".into()
        };
        let loc = location_for(card, &row.file, &row.symbol);
        let line = format!(
            "  {:>4}  {:>3}  {:>4}  {:<16}  {}",
            fmt_crap(row.crap),
            row.cc,
            cov,
            fit(&row.symbol, 16, opts.color),
            loc
        );
        out.push_str(&fit(&line, width, opts.color));
        out.push('\n');
    }
    out.push('\n');
}

fn push_findings(out: &mut String, card: &Scorecard, opts: &PrettyOpts, width: usize) {
    out.push_str("findings\n");
    if card.findings.is_empty() {
        out.push_str("  (none)\n\n");
        return;
    }
    let mut last = String::new();
    for finding in card.findings.iter().take(FINDING_LIMIT) {
        if finding.severity != last {
            out.push_str(&paint(
                opts.color,
                severity_style(&finding.severity),
                &finding.severity,
            ));
            out.push('\n');
            last = finding.severity.clone();
        }
        out.push_str(&finding_block(finding, opts, width));
    }
    if card.findings.len() > FINDING_LIMIT {
        let more = card.findings.len() - FINDING_LIMIT;
        out.push_str(&format!("  {more} more, see --out report\n"));
    }
    out.push('\n');
}

fn finding_block(finding: &Finding, opts: &PrettyOpts, width: usize) -> String {
    let mut block = format!("  {}\n", finding.rule);
    let place = finding_place(finding);
    block.push_str("  ");
    block.push_str(&fit(&place, width.saturating_sub(2), opts.color));
    block.push('\n');
    if let Some(action) = finding.suggested_action.as_deref() {
        if !action.is_empty() {
            block.push_str("  ");
            block.push_str(&fit(action, width.saturating_sub(2), opts.color));
            block.push('\n');
        }
    }
    block
}

fn push_llm(out: &mut String, card: &Scorecard, opts: &PrettyOpts, width: usize) {
    let Some(section) = card.llm.as_ref() else {
        return;
    };
    out.push_str("llm\n");
    if section.status == "skipped" {
        out.push_str("  skipped: ");
        out.push_str(section.reason.as_deref().unwrap_or("llm did not run"));
        out.push_str("\n\n");
        return;
    }
    let mut facts = String::new();
    if let Some(backend) = section.backend.as_deref() {
        facts.push_str("  backend ");
        facts.push_str(backend);
    }
    if let Some(model) = section.model.as_deref() {
        if !facts.is_empty() {
            facts.push_str("  ");
        }
        facts.push_str("model ");
        facts.push_str(model);
    }
    if let Some(rounds) = section.rounds {
        facts.push_str(&format!("  rounds {rounds}"));
    }
    if !facts.is_empty() {
        out.push_str(&fit(&facts, width, opts.color));
        out.push('\n');
    }
    if let Some(verdict) = section.verdict.as_deref() {
        out.push_str(&fit(&format!("  verdict: {verdict}"), width, opts.color));
        out.push('\n');
    }
    for note in section.notes.iter().take(10) {
        out.push_str(&fit(&format!("  - {note}"), width, opts.color));
        out.push('\n');
    }
    out.push('\n');
}

fn push_footer(out: &mut String, card: &Scorecard, opts: &PrettyOpts) {
    out.push_str("engines run: ");
    out.push_str(&list_or_none(&card.engines_run));
    out.push('\n');
    out.push_str("engines skipped: ");
    out.push_str(&list_or_none(&card.engines_skipped));
    out.push('\n');
    out.push_str(&format!("duration: {}\n", fmt_duration(total_ms(card))));
    if let Some(path) = opts.report.as_deref() {
        out.push_str("report: ");
        out.push_str(path);
        out.push('\n');
    }
    out.push_str(&exit_line(
        opts.exit_code,
        matches!(Outcome::of(card), Outcome::ReportOnly(_)),
    ));
}

fn exit_line(code: i32, report_only: bool) -> String {
    let meaning = match code {
        0 if report_only => "no enforced gate failed",
        0 => "gates passed",
        1 => "a gate failed",
        _ => "the analyzer could not finish",
    };
    format!("exit {code}: {meaning}")
}

fn pack_name(card: &Scorecard) -> &str {
    if card.pack.is_empty() {
        "unknown"
    } else {
        &card.pack
    }
}

fn short_sha(head: &Option<String>) -> String {
    match head {
        Some(sha) if sha.len() >= 7 => sha[..7].to_string(),
        Some(sha) if !sha.is_empty() => sha.clone(),
        _ => "none".to_string(),
    }
}

fn mark(pass: bool, color: bool) -> &'static str {
    if color {
        if pass {
            "✓"
        } else {
            "✗"
        }
    } else if pass {
        "[ok]"
    } else {
        "[xx]"
    }
}

fn bar(score: f64) -> String {
    let filled = (score.clamp(0.0, 1.0) * 10.0).round() as usize;
    let filled = filled.min(10);
    format!("[{}{}]", "#".repeat(filled), ".".repeat(10 - filled))
}

fn fmt_crap(value: f64) -> String {
    if (value - value.round()).abs() < 1e-6 {
        format!("{}", value.round() as i64)
    } else {
        format!("{value:.1}")
    }
}

fn fmt_duration(ms: u64) -> String {
    format!("{:.1}s", ms as f64 / 1000.0)
}

fn total_ms(card: &Scorecard) -> u64 {
    card.runs.iter().map(|run| run.duration_ms).sum()
}

fn list_or_none(items: &[String]) -> String {
    if items.is_empty() {
        "(none)".to_string()
    } else {
        items.join(", ")
    }
}

fn location_for(card: &Scorecard, file: &str, symbol: &str) -> String {
    let line = card
        .findings
        .iter()
        .find(|finding| finding.file == file && finding.symbol.as_deref() == Some(symbol));
    match line.and_then(|finding| finding.span.as_ref()) {
        Some(span) => format!("{file}:{}", span.start_line),
        None => file.to_string(),
    }
}

fn finding_place(finding: &Finding) -> String {
    let loc = match finding.span.as_ref() {
        Some(span) => format!("{}:{}", finding.file, span.start_line),
        None => finding.file.clone(),
    };
    match finding.symbol.as_deref() {
        Some(symbol) if !symbol.is_empty() => format!("{loc}  {symbol}"),
        _ => loc,
    }
}

fn fit(text: &str, width: usize, unicode: bool) -> String {
    let width = width.max(1);
    if text.chars().count() <= width {
        return text.to_string();
    }
    let keep = width - 1;
    let mut out: String = text.chars().take(keep).collect();
    out.push(if unicode { '…' } else { '~' });
    out
}

fn paint(color: bool, style: Style, text: &str) -> String {
    if !color {
        return text.to_string();
    }
    format!("{}{text}{}", style.render(), style.render_reset())
}

fn green() -> Style {
    Style::new().fg_color(Some(Color::Ansi(AnsiColor::Green)))
}

fn blue() -> Style {
    Style::new().fg_color(Some(Color::Ansi(AnsiColor::Blue)))
}

fn red() -> Style {
    Style::new().fg_color(Some(Color::Ansi(AnsiColor::Red)))
}

fn dim() -> Style {
    Style::new().fg_color(Some(Color::Ansi(AnsiColor::BrightBlack)))
}

fn severity_style(severity: &str) -> Style {
    match severity {
        "error" => red().bold(),
        "warning" => Style::new()
            .bold()
            .fg_color(Some(Color::Ansi(AnsiColor::Yellow))),
        _ => Style::new().bold(),
    }
}

fn unix_cols() -> Option<usize> {
    #[cfg(unix)]
    {
        unsafe {
            // SAFETY: winsize is a plain C struct and the pointer is only
            // read by this ioctl for the life of the call.
            let mut size = libc::winsize {
                ws_row: 0,
                ws_col: 0,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            let rc = libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut size);
            if rc == 0 && size.ws_col as usize >= MIN_WIDTH {
                return Some(size.ws_col as usize);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::LlmSection;

    #[test]
    fn pretty_shows_llm_notes_and_a_plain_skip_reason() {
        let opts = PrettyOpts {
            color: false,
            width: 80,
            version: "0.1.0".into(),
            report: None,
            exit_code: 0,
        };
        let mut card = Scorecard::skeleton("demo", 30);
        let off = to_pretty(&card, &opts);
        assert!(!off.contains("\nllm\n"));
        card.llm = Some(LlmSection {
            status: "ran".into(),
            backend: Some("ollama".into()),
            model: Some("qwen2.5-coder".into()),
            rounds: None,
            verdict: None,
            notes: vec!["n".repeat(80)],
            reason: None,
        });
        let narrow = PrettyOpts { width: 40, ..opts };
        let missing = to_pretty(&card, &narrow);
        assert!(!missing.contains("no gaps"));
        assert!(!missing.contains("rounds"));
        assert!(missing.contains('~'));
        card.llm = Some(LlmSection::ran(
            "ollama",
            "qwen2.5-coder",
            3,
            0,
            vec!["checked the intent against src/lib.rs".into()],
        ));
        card.engines_skipped.retain(|engine| engine != "llm");
        card.engines_run.push("llm".into());
        let wide = PrettyOpts {
            color: false,
            width: 80,
            version: "0.1.0".into(),
            report: None,
            exit_code: 0,
        };
        let ran = to_pretty(&card, &wide);
        assert!(ran.contains("verdict: no gaps"));
        assert!(ran.contains("checked the intent against src/lib.rs"));
        assert!(ran.contains("backend ollama"));
    }

    #[test]
    fn omitted_format_is_pretty_on_a_tty_and_json_otherwise() {
        assert_eq!(stdout_format(None, true), "pretty");
        assert_eq!(stdout_format(None, false), "json");
        assert_eq!(stdout_format(Some("md"), true), "md");
        assert_eq!(stdout_format(Some("json"), true), "json");
        assert_eq!(stdout_format(Some("pretty"), false), "pretty");
    }

    #[test]
    fn color_follows_no_color_force_and_the_tty() {
        assert!(!use_color(true, true, Some("1"), None));
        assert!(use_color(false, false, Some("1"), None));
        assert!(!use_color(true, false, Some("0"), None));
        assert!(!use_color(true, false, None, Some("0")));
        assert!(use_color(true, false, None, None));
        assert!(!use_color(false, false, None, None));
    }

    #[test]
    fn plain_banner_has_no_escapes() {
        let card = Scorecard::skeleton("demo", 30);
        let mut card = card;
        card.verdict = "pass".into();
        card.pack = "rust".into();
        let text = to_pretty(
            &card,
            &PrettyOpts {
                color: false,
                width: 80,
                version: "0.1.0".into(),
                report: None,
                exit_code: 0,
            },
        );
        assert!(text.contains("PASS"));
        assert!(!text.contains('\u{1b}'));
        assert!(text.contains("[ok]") || text.contains("(none)"));
        assert!(text.contains("exit 0: gates passed"));
    }

    #[test]
    fn report_only_banner_replaces_pass() {
        let mut card = Scorecard::skeleton("demo", 30);
        card.verdict = "pass".into();
        card.gates.push(Gate {
            id: "crap".into(),
            pass: false,
            enforced: false,
            reason: Some("1 function over threshold".into()),
        });
        let text = to_pretty(
            &card,
            &PrettyOpts {
                color: false,
                width: 80,
                version: "0.1.0".into(),
                report: None,
                exit_code: 0,
            },
        );
        assert!(text.contains("REPORT ONLY  1 failing gate, none enforced"));
        assert!(!text.contains("PASS"));
        assert!(text.contains("exit 0: no enforced gate failed"));
    }

    #[test]
    fn advisory_miss_keeps_the_pass_banner() {
        let mut card = Scorecard::skeleton("demo", 30);
        card.verdict = "pass".into();
        card.gates = vec![
            Gate {
                id: "types".into(),
                pass: true,
                enforced: true,
                reason: None,
            },
            Gate {
                id: "sca".into(),
                pass: false,
                enforced: false,
                reason: Some("3 undeclared dependencies".into()),
            },
        ];
        let text = to_pretty(
            &card,
            &PrettyOpts {
                color: false,
                width: 80,
                version: "0.1.0".into(),
                report: None,
                exit_code: 0,
            },
        );
        assert!(text.contains("PASS  1 advisory gate failing"));
        assert!(!text.contains("REPORT ONLY"));
        assert!(text.contains("exit 0: gates passed"));
    }

    #[test]
    fn crap_rows_show_dashes_when_coverage_was_not_measured() {
        let mut card = Scorecard::skeleton("demo", 30);
        card.verdict = "pass".into();
        card.crap.worst = vec![CrapFunction {
            symbol: "classify".into(),
            file: "src/lib.rs".into(),
            cc: 11,
            coverage: 0.0,
            crap: 132.0,
        }];
        let opts = PrettyOpts {
            color: false,
            width: 120,
            version: "0.1.0".into(),
            report: None,
            exit_code: 0,
        };
        let text = to_pretty(&card, &opts);
        assert!(text.contains("coverage not measured: CRAP assumes 0% coverage (upper bound)"));
        assert!(text.contains("   132   11    --  classify"));
        card.engines_run.push("coverage".into());
        let text = to_pretty(&card, &opts);
        assert!(text.contains("   132   11    0%  classify"));
        assert!(!text.contains("not measured"));
    }

    #[test]
    fn diff_scope_line_fits_80_columns() {
        let mut card = Scorecard::skeleton("demo", 30);
        card.verdict = "pass".into();
        card.scope.mode = "diff".into();
        card.scope.base = Some("origin/develop".into());
        card.metrics.crap_over_threshold = 12;
        let text = to_pretty(
            &card,
            &PrettyOpts {
                color: false,
                width: 80,
                version: "0.1.0".into(),
                report: None,
                exit_code: 0,
            },
        );
        let line = "  diff scope: 12 over threshold here; tree count: latest develop push run";
        assert!(text.contains(&format!("worst crap  threshold 30\n{line}\n")));
        assert!(line.len() <= 80);
    }
}

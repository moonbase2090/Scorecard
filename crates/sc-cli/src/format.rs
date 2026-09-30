// SPDX-License-Identifier: MPL-2.0
use crate::report::Outcome;
use sc_core::Scorecard;

/// Render a scorecard as pretty JSON.
///
/// ```
/// use sc_cli::to_json;
/// use sc_core::Scorecard;
/// let card = Scorecard::skeleton("demo", 30);
/// let value: serde_json::Value = serde_json::from_str(&to_json(&card)).unwrap();
/// assert_eq!(value["repo"], "demo");
/// assert_eq!(value["verdict"], "fail");
/// ```
pub fn to_json(card: &Scorecard) -> String {
    let mut report = card.clone();
    report.verdict = sc_core::report_verdict(card).to_string();
    serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string())
}

/// Render a scorecard as Markdown.
///
/// ```
/// use sc_cli::to_markdown;
/// use sc_core::Scorecard;
/// let card = Scorecard::skeleton(".", 30);
/// let md = to_markdown(&card);
/// assert!(md.starts_with("# scorecard\n"));
/// assert!(md.contains("**Verdict:** fail"));
/// assert!(md.contains("**Repo:** ."));
/// ```
fn push_llm_markdown(out: &mut String, card: &Scorecard) {
    let Some(section) = card.llm.as_ref() else {
        return;
    };
    out.push_str("## LLM\n\n");
    if section.status == "skipped" {
        out.push_str("Skipped: ");
        out.push_str(section.reason.as_deref().unwrap_or("llm did not run"));
        out.push_str(".\n\n");
        return;
    }
    if let Some(backend) = section.backend.as_deref() {
        out.push_str(&format!("- backend: {backend}\n"));
    }
    if let Some(model) = section.model.as_deref() {
        out.push_str(&format!("- model: {model}\n"));
    }
    if let Some(rounds) = section.rounds {
        out.push_str(&format!("- rounds: {rounds}\n"));
    }
    if let Some(verdict) = section.verdict.as_deref() {
        out.push_str(&format!("- verdict: {verdict}\n"));
    }
    if !section.notes.is_empty() {
        out.push('\n');
        for note in section.notes.iter().take(10) {
            out.push_str(&format!("- {note}\n"));
        }
    }
    out.push('\n');
}

pub fn to_markdown(card: &Scorecard) -> String {
    let mut out = String::new();
    push_markdown_header(&mut out, card);
    push_llm_markdown(&mut out, card);
    push_markdown_gates(&mut out, card);
    push_markdown_scores(&mut out, card);
    push_markdown_crap(&mut out, card);
    push_markdown_generated_files(&mut out, card);
    push_markdown_findings(&mut out, card);
    out
}

fn push_markdown_generated_files(out: &mut String, card: &Scorecard) {
    let Some(warning) = card.generated_files_warning.as_ref() else {
        return;
    };
    out.push_str("## Generated files analyzed\n\n");
    for file in &warning.files {
        out.push_str(&format!("- `{}`\n", cell(file)));
    }
    out.push_str(&format!("\n{}\n\n", warning.suggested_action));
}

fn push_markdown_header(out: &mut String, card: &Scorecard) {
    let head = card.git.head.as_deref().unwrap_or("none");
    let dirty = escape_markdown(&card.git.status_label());
    out.push_str("# scorecard\n\n");
    let outcome = Outcome::of(card);
    let verdict = sc_core::report_verdict(card);
    match (outcome, outcome.note()) {
        (Outcome::ReportOnly(_), Some(note)) => out.push_str(&format!(
            "**Verdict:** {} (report only: {note})\n\n",
            verdict
        )),
        (_, Some(note)) => out.push_str(&format!("**Verdict:** {} ({note})\n\n", verdict)),
        (_, None) => out.push_str(&format!("**Verdict:** {}\n\n", verdict)),
    }
    out.push_str(&format!("**Repo:** {}\n\n", card.repo));
    out.push_str(&format!("**Git:** {head} ({dirty})\n\n"));
    let changed_paths = card.git.changed_paths_label();
    if !changed_paths.is_empty() {
        out.push_str(&format!(
            "**Changed paths:** {}\n\n",
            escape_markdown(&changed_paths)
        ));
    }
    let paths = match card.scope.paths.len() {
        0 => String::new(),
        1 => ", 1 path".into(),
        n => format!(", {n} paths"),
    };
    let rest = match crate::report::rest_of_tree(card) {
        Some(line) => format!(" {line}."),
        None => String::new(),
    };
    out.push_str(&format!(
        "**Scope:** {}{paths}.{rest} `loc_changed`, `files_changed`, and `coverage_changed` describe that scope.\n\n",
        card.scope.mode
    ));
    if let Some(intent) = &card.intent {
        if !intent.trim().is_empty() {
            out.push_str(&format!("**Intent:** {}\n\n", intent.trim()));
        }
    }
    out.push_str(&format!(
        "**Engines run:** {}\n\n",
        card.engines_run.join(", ")
    ));
    if !card.engines_skipped.is_empty() {
        out.push_str(&format!(
            "**Engines skipped:** {}\n\n",
            card.engines_skipped.join(", ")
        ));
    }
}

fn escape_markdown(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(ch, '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '|') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

fn push_markdown_gates(out: &mut String, card: &Scorecard) {
    out.push_str("## Gates\n\n");
    out.push_str("| Gate | Result | Enforced | Reason |\n|---|---|---|---|\n");
    for gate in &card.gates {
        let result = if gate.pass { "pass" } else { "fail" };
        let enforced = if gate.enforced { "yes" } else { "no" };
        let reason = gate.reason.as_deref().unwrap_or("");
        out.push_str(&format!(
            "| {} | {result} | {enforced} | {} |\n",
            cell(&gate.id),
            cell(reason)
        ));
    }
    out.push('\n');
}

fn push_markdown_scores(out: &mut String, card: &Scorecard) {
    out.push_str("## Scores\n\n");
    out.push_str(&format!(
        "- correctness: {:.2}\n- efficiency: {:.2}\n- maintainability: {:.2}\n- security: {:.2}\n- a11y: {:.2}\n\n",
        card.scores.correctness,
        card.scores.efficiency,
        card.scores.maintainability,
        card.scores.security,
        card.scores.a11y
    ));
}

fn push_markdown_crap(out: &mut String, card: &Scorecard) {
    out.push_str("## Worst CRAP\n\n");
    out.push_str(&format!("Threshold {}.\n\n", card.crap.threshold));
    if let Some(line) = crate::report::rest_of_tree(card) {
        out.push_str(&line);
        out.push_str("\n\n");
    }
    if let Some(line) = crate::report::diff_baseline(card) {
        out.push_str(&line);
        out.push_str("\n\n");
    }
    let measured = crate::report::coverage_measured(card);
    if !measured {
        out.push_str(crate::report::COVERAGE_NOT_MEASURED);
        out.push_str("\n\n");
    }
    out.push_str("| CRAP | CC | Coverage | Symbol | File |\n|---|---|---|---|---|\n");
    for row in &card.crap.worst {
        let coverage = if measured {
            format!("{}%", pct(row.coverage))
        } else {
            "not measured".into()
        };
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            fmt_num(row.crap),
            row.cc,
            coverage,
            cell(&row.symbol),
            cell(&row.file)
        ));
    }
    if card.crap.worst.is_empty() {
        out.push_str("| | | | | |\n");
    }
    out.push('\n');
}

fn push_markdown_findings(out: &mut String, card: &Scorecard) {
    out.push_str("## Findings\n\n");
    if card.findings.is_empty() {
        out.push_str("None.\n");
    }
    for finding in &card.findings {
        out.push_str(&format!("### {}\n\n", finding.id));
        out.push_str(&format!("- rule: `{}`\n", finding.rule));
        out.push_str(&format!("- severity: {}\n", finding.severity));
        if !finding.disposition.is_empty() {
            out.push_str(&format!("- disposition: {}\n", finding.disposition));
        }
        out.push_str(&format!("- engine: {}\n", finding.engine));
        match &finding.span {
            Some(span) => out.push_str(&format!(
                "- location: {}:{}:{}\n",
                finding.file, span.start_line, span.start_col
            )),
            None => out.push_str(&format!("- file: {}\n", finding.file)),
        }
        if let Some(symbol) = &finding.symbol {
            out.push_str(&format!("- symbol: `{symbol}`\n"));
        }
        out.push_str(&format!("- {}\n", finding.message));
        let (functions, more) = crate::report::evidence_functions(finding);
        if !functions.is_empty() {
            out.push_str("- functions:\n");
            for (location, symbol) in &functions {
                out.push_str(&format!("  - `{location}` `{symbol}`\n"));
            }
            if more > 0 {
                out.push_str(&format!("  - {more} more\n"));
            }
        }
        if let Some(log) = crate::report::raw_log(finding) {
            let fence = "`".repeat(longest_backtick_run(log).max(2) + 1);
            out.push_str(&format!(
                "\n<details><summary>full output</summary>\n\n{fence}text\n{log}\n{fence}\n\n</details>\n\n"
            ));
        }
        if let Some(action) = &finding.suggested_action {
            out.push_str(&format!("- suggested: {action}\n"));
        }
        out.push('\n');
    }
}

/// A fence one backtick longer than any run in the log keeps it closed.
fn longest_backtick_run(text: &str) -> usize {
    text.split(|c| c != '`').map(str::len).max().unwrap_or(0)
}

fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

fn pct(coverage: f64) -> i64 {
    (coverage.clamp(0.0, 1.0) * 100.0).round() as i64
}

fn fmt_num(value: f64) -> String {
    if (value - value.round()).abs() < 1e-6 {
        format!("{}", value.round() as i64)
    } else {
        format!("{value:.1}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::{
        CrapFunction, CrapSection, Finding, Gate, GeneratedFilesWarning, LlmSection, Scorecard,
    };

    #[test]
    fn generated_files_warning_appears_in_every_report_format() {
        let mut card = Scorecard::skeleton("demo", 30);
        card.generated_files_warning = Some(
            GeneratedFilesWarning::from_paths(vec![
                "build/parser.rs".into(),
                "src/generated.rs".into(),
            ])
            .unwrap(),
        );

        let json: serde_json::Value = serde_json::from_str(&to_json(&card)).unwrap();
        assert_eq!(
            json["generated_files_warning"]["files"][0],
            "build/parser.rs"
        );
        assert!(json["generated_files_warning"]["suggested_action"]
            .as_str()
            .unwrap()
            .contains("scope.exclude"));

        let md = to_markdown(&card);
        assert!(md.contains("## Generated files analyzed"));
        assert!(md.contains("`build/parser.rs`"));
        assert!(md.contains("scope.exclude"));

        let html = crate::report::to_html(&card);
        assert!(html.contains("generated files analyzed"));
        assert!(html.contains("build/parser.rs"));
        assert!(html.contains("scope.exclude"));

        let pretty = crate::pretty::to_pretty(
            &card,
            &crate::pretty::PrettyOpts {
                color: false,
                width: 100,
                version: "0.1.0".into(),
                report: None,
                exit_code: 0,
            },
        );
        assert!(pretty.contains("generated files analyzed"));
        assert!(pretty.contains("build/parser.rs"));
        assert!(pretty.contains("scope.exclude"));
    }

    #[test]
    fn markdown_shows_llm_notes_and_a_plain_skip_reason() {
        let mut card = Scorecard::skeleton("demo", 30);
        let off = to_markdown(&card);
        assert!(!off.contains("## LLM"));
        card.llm = Some(LlmSection::skipped(
            "llm is on, but neither --spec nor --intent was given. Re-run with --spec PATH or --intent TEXT.",
        ));
        let skipped = to_markdown(&card);
        assert!(skipped.contains("neither --spec nor --intent"));
        card.llm = Some(LlmSection::ran(
            "ollama",
            "qwen2.5-coder",
            2,
            1,
            vec!["the intent's SHA clip is missing".into()],
        ));
        let ran = to_markdown(&card);
        assert!(ran.contains("verdict: gaps found"));
        assert!(ran.contains("the intent's SHA clip is missing"));
    }

    #[test]
    fn markdown_marks_unmeasured_coverage() {
        let mut card = Scorecard::skeleton(".", 30);
        let md = to_markdown(&card);
        assert!(md.contains(
            "Coverage was not measured, so no CRAP scores are reported and nothing is treated as 0% coverage."
        ));
        assert!(md.contains("| | | | | |"));
        card.engines_run.push("coverage".into());
        card.crap = CrapSection {
            threshold: 30,
            worst: vec![CrapFunction {
                symbol: "classify".into(),
                file: "src/lib.rs".into(),
                cc: 11,
                coverage: 0.0,
                crap: 132.0,
            }],
        };
        let md = to_markdown(&card);
        assert!(md.contains("| 132 | 11 | 0% | classify | src/lib.rs |"));
        assert!(!md.contains("not measured"));
    }

    #[test]
    fn markdown_keeps_measured_coverage_when_some_functions_lack_a_record() {
        let mut card = Scorecard::skeleton(".", 30);
        card.engines_run.push("coverage".into());
        card.gates.push(Gate {
            id: "crap".into(),
            pass: false,
            enforced: false,
            reason: Some("some functions have no coverage record and were not scored".into()),
        });
        card.crap = CrapSection {
            threshold: 30,
            worst: vec![CrapFunction {
                symbol: "classify".into(),
                file: "src/lib.rs".into(),
                cc: 11,
                coverage: 0.4,
                crap: 40.0,
            }],
        };
        let md = to_markdown(&card);
        assert!(md.contains("| 40 | 11 | 40% | classify | src/lib.rs |"));
        assert!(!md.contains("not measured"));
        assert!(!md.contains("CRAP is not scored"));
    }

    #[test]
    fn markdown_folds_the_raw_log_with_a_safe_fence() {
        let mut card = Scorecard::skeleton(".", 30);
        card.findings.push(Finding {
            id: "tests:failed".into(),
            rule: "test.failed".into(),
            engine: "tests".into(),
            severity: "error".into(),
            file: "tests/test_x.py".into(),
            span: None,
            symbol: Some("test_x".into()),
            message: "FAILED tests/test_x.py::test_x".into(),
            evidence: serde_json::json!({"log": "....F\n```\nFAILED tests/test_x.py::test_x"}),
            suggested_action: None,
            disposition: String::new(),
        });
        let md = to_markdown(&card);
        assert!(md.contains("- FAILED tests/test_x.py::test_x\n\n<details><summary>full output</summary>\n\n````text\n....F\n```\nFAILED"));
        assert!(md.contains("\n````\n\n</details>"));
    }

    #[test]
    fn markdown_rest_of_tree_names_paths_outside_the_diff() {
        let mut card = Scorecard::skeleton(".", 30);
        card.scope.mode = "diff".into();
        card.scope.paths = vec!["src/a.rs".into()];
        card.scope.other_paths = Some(11);
        card.scope.base = Some("main".into());
        let md = to_markdown(&card);
        assert!(md.contains("1 path in this diff; 11 other paths in the tree"));
        assert!(md
            .contains("**Scope:** diff, 1 path. 1 path in this diff; 11 other paths in the tree."));
    }

    #[test]
    fn markdown_diff_scope_names_the_diff_count() {
        let mut card = Scorecard::skeleton(".", 30);
        card.scope.mode = "diff".into();
        card.scope.base = Some("main".into());
        card.metrics.crap_over_threshold = 2;
        let md = to_markdown(&card);
        assert!(md.contains("## Worst CRAP\n\nThreshold 30.\n\ndiff scope: 2 functions over threshold in this diff. The tree-wide count is on the latest push run of main.\n"));
    }

    #[test]
    fn markdown_lists_evidence_functions() {
        let mut card = Scorecard::skeleton(".", 30);
        card.findings.push(Finding {
            id: "coverage:unmatched".into(),
            rule: "coverage.unmatched".into(),
            engine: "coverage".into(),
            severity: "warning".into(),
            file: ".".into(),
            span: None,
            symbol: None,
            message: "2 analyzed function(s) had no llvm-cov record".into(),
            evidence: serde_json::json!({
                "unmatched": 3,
                "functions": [
                    {"file": "src/a.rs", "symbol": "a::run", "line": 12},
                    {"file": "src/b.rs", "symbol": "b::go"},
                ],
            }),
            suggested_action: None,
            disposition: String::new(),
        });
        let md = to_markdown(&card);
        assert!(md.contains(
            "- functions:\n  - `src/a.rs:12` `a::run`\n  - `src/b.rs` `b::go`\n  - 1 more\n"
        ));
    }

    #[test]
    fn markdown_includes_verdict_and_crap() {
        let mut card = Scorecard::skeleton(".", 30);
        card.verdict = "pass".into();
        card.gates.push(Gate {
            id: "crap".into(),
            pass: false,
            enforced: true,
            reason: Some("1 function over threshold".into()),
        });
        card.crap = CrapSection {
            threshold: 30,
            worst: vec![CrapFunction {
                symbol: "classify".into(),
                file: "src/lib.rs".into(),
                cc: 11,
                coverage: 0.0,
                crap: 132.0,
            }],
        };
        card.findings.push(Finding {
            id: "crap:src/lib.rs:classify".into(),
            rule: "crap.over_threshold".into(),
            engine: "crap".into(),
            severity: "error".into(),
            file: "src/lib.rs".into(),
            span: None,
            symbol: Some("classify".into()),
            message: "CRAP 132 (CC=11, cov=0%) exceeds threshold 30".into(),
            evidence: serde_json::json!({}),
            suggested_action: Some("Add tests covering branches or split the function".into()),
            disposition: String::new(),
        });
        let md = to_markdown(&card);
        assert!(md.contains("**Verdict:** fail"));
        assert!(md.contains("classify"));
        assert!(md.contains("crap.over_threshold"));
    }

    #[test]
    fn json_does_not_pass_when_an_enforced_gate_fails() {
        let mut card = Scorecard::skeleton(".", 30);
        card.verdict = "pass".into();
        card.gates.push(Gate {
            id: "lint".into(),
            pass: false,
            enforced: true,
            reason: Some("lint failed".into()),
        });
        let value: serde_json::Value = serde_json::from_str(&to_json(&card)).unwrap();
        assert_eq!(value["verdict"], "fail");
        assert_eq!(value["gates"][0]["enforced"], true);
    }

    #[test]
    fn markdown_marks_pass_with_failures_report_only() {
        let mut card = Scorecard::skeleton(".", 30);
        card.verdict = "pass".into();
        card.gates.push(Gate {
            id: "crap".into(),
            pass: false,
            enforced: false,
            reason: Some("1 function over threshold".into()),
        });
        let md = to_markdown(&card);
        assert!(md.contains("**Verdict:** pass (report only: 1 failing gate, none enforced)"));
        assert!(!md.contains("of `src`"));
    }

    #[test]
    fn markdown_renders_dirty_git_intent_and_a_located_finding() {
        let mut card = Scorecard::skeleton("demo", 30);
        card.git.head = Some("abc123".into());
        card.git.dirty = true;
        card.intent = Some("  keep the header contrast  ".into());
        card.scope.paths = vec!["src/lib.rs".into()];
        card.engines_skipped.clear();
        card.engines_run.push("coverage".into());
        card.llm = Some(LlmSection {
            status: "skipped".into(),
            backend: None,
            model: None,
            rounds: None,
            verdict: None,
            notes: Vec::new(),
            reason: None,
        });
        card.gates.push(Gate {
            id: "crap|gate".into(),
            pass: false,
            enforced: true,
            reason: Some("line one\nstill failing".into()),
        });
        card.crap.worst.push(CrapFunction {
            symbol: "render".into(),
            file: "src/lib.rs".into(),
            cc: 4,
            coverage: 0.5,
            crap: 6.5,
        });
        card.findings.push(Finding {
            id: "crap:src/lib.rs:render".into(),
            rule: "crap.over_threshold".into(),
            engine: "crap".into(),
            severity: "error".into(),
            file: "src/lib.rs".into(),
            span: Some(sc_core::Span {
                start_line: 12,
                start_col: 3,
                end_line: 20,
                end_col: 1,
            }),
            symbol: Some("render".into()),
            message: "over the line".into(),
            evidence: serde_json::json!({}),
            suggested_action: Some("Add a test".into()),
            disposition: "fix".into(),
        });
        let md = to_markdown(&card);
        assert!(md.contains("**Git:** abc123 (dirty)"));
        assert!(md.contains("**Intent:** keep the header contrast"));
        assert!(md.contains(", 1 path"));
        assert!(!md.contains("**Engines skipped:**"));
        assert!(md.contains("Skipped: llm did not run."));
        assert!(md.contains("| crap\\|gate | fail | yes | line one still failing |"));
        assert!(md.contains("| 6.5 | 4 | 50% | render | src/lib.rs |"));
        assert!(md.contains("- location: src/lib.rs:12:3"));
        assert!(md.contains("- disposition: fix"));

        card.llm = Some(LlmSection {
            status: "ran".into(),
            backend: None,
            model: None,
            rounds: None,
            verdict: None,
            notes: (0..12).map(|index| format!("note {index}")).collect(),
            reason: None,
        });
        card.scope.paths = (0..3).map(|index| format!("f{index}.rs")).collect();
        let md = to_markdown(&card);
        assert!(md.contains(", 3 paths"));
        assert!(md.contains("- note 9"));
        assert!(!md.contains("- note 10"));
        assert!(!md.contains("- backend:"));
    }

    #[test]
    fn markdown_notes_advisory_misses_on_a_pass() {
        let mut card = Scorecard::skeleton(".", 30);
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
        let md = to_markdown(&card);
        assert!(md.contains("**Verdict:** pass (1 advisory gate failing)"));
        assert!(!md.contains("report only"));
    }

    #[test]
    fn sca_enforced_flag_matches_across_formats() {
        let mut card = Scorecard::skeleton("demo", 30);
        card.verdict = "pass".into();
        card.gates = vec![
            Gate {
                id: "types".into(),
                pass: true,
                reason: None,
                enforced: true,
            },
            Gate {
                id: "sca".into(),
                pass: true,
                reason: None,
                enforced: false,
            },
        ];
        let json: serde_json::Value = serde_json::from_str(&to_json(&card)).unwrap();
        assert_eq!(json["gates"][0]["enforced"], true);
        assert_eq!(json["gates"][1]["enforced"], false);

        let md = to_markdown(&card);
        assert!(md.contains("| types | pass | yes |"));
        assert!(md.contains("| sca | pass | no |"));

        let html = crate::report::to_html(&card);
        let types_at = html.find(">types<").unwrap();
        let sca_at = html.find(">sca<").unwrap();
        let types_cell = &html[types_at..html[types_at..].find("</tr>").unwrap() + types_at];
        let sca_cell = &html[sca_at..html[sca_at..].find("</tr>").unwrap() + sca_at];
        assert!(types_cell.contains(">yes<"), "{types_cell}");
        assert!(sca_cell.contains("reported only"), "{sca_cell}");
        assert!(html.contains("overflow-wrap:anywhere"));

        let pretty = crate::pretty::to_pretty(
            &card,
            &crate::pretty::PrettyOpts {
                color: false,
                width: 80,
                version: "0.1.0".into(),
                report: None,
                exit_code: 0,
            },
        );
        assert!(pretty.contains("types") && pretty.contains("enforced"));
        assert!(pretty.contains("sca") && pretty.contains("advisory"));
        assert!(!pretty
            .lines()
            .any(|line| line.contains("sca") && line.contains("enforced")));

        let sarif: serde_json::Value = serde_json::from_str(&sc_sarif::to_sarif(&card)).unwrap();
        assert_eq!(
            sarif["runs"][0]["properties"]["scorecardGates"][0]["enforced"],
            true
        );
        assert_eq!(
            sarif["runs"][0]["properties"]["scorecardGates"][1]["enforced"],
            false
        );
    }
}

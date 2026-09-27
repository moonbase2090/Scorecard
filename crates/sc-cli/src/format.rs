// SPDX-License-Identifier: MPL-2.0
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
    serde_json::to_string_pretty(card).unwrap_or_else(|_| "{}".to_string())
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
pub fn to_markdown(card: &Scorecard) -> String {
    let mut out = String::new();
    let head = card.git.head.as_deref().unwrap_or("none");
    let dirty = if card.git.dirty { "dirty" } else { "clean" };
    out.push_str("# scorecard\n\n");
    out.push_str(&format!("**Verdict:** {}\n\n", card.verdict));
    out.push_str(&format!("**Repo:** {}\n\n", card.repo));
    out.push_str(&format!("**Git:** {head} ({dirty})\n\n"));
    out.push_str(&format!(
        "**Scope:** {} of `src`. `loc_changed`, `files_changed`, and `coverage_changed` describe that tree.\n\n",
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

    out.push_str("## Scores\n\n");
    out.push_str(&format!(
        "- correctness: {:.2}\n- efficiency: {:.2}\n- maintainability: {:.2}\n- security: {:.2}\n\n",
        card.scores.correctness,
        card.scores.efficiency,
        card.scores.maintainability,
        card.scores.security
    ));

    out.push_str("## Worst CRAP\n\n");
    out.push_str(&format!("Threshold {}.\n\n", card.crap.threshold));
    out.push_str("| CRAP | CC | Coverage | Symbol | File |\n|---|---|---|---|---|\n");
    for row in &card.crap.worst {
        out.push_str(&format!(
            "| {} | {} | {}% | {} | {} |\n",
            fmt_num(row.crap),
            row.cc,
            pct(row.coverage),
            cell(&row.symbol),
            cell(&row.file)
        ));
    }
    if card.crap.worst.is_empty() {
        out.push_str("| | | | | |\n");
    }
    out.push('\n');

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
        if let Some(action) = &finding.suggested_action {
            out.push_str(&format!("- suggested: {action}\n"));
        }
        out.push('\n');
    }
    out
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
    use sc_core::{CrapFunction, CrapSection, Finding, Gate, Scorecard};

    #[test]
    fn markdown_includes_verdict_and_crap() {
        let mut card = Scorecard::skeleton(".", 30);
        card.verdict = "fail".into();
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

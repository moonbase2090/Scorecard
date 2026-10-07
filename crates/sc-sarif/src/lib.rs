// SPDX-License-Identifier: MPL-2.0
//! SARIF 2.1.0 export of a scorecard.

use sc_core::{Finding, Scorecard};
use serde_json::{json, Value};

pub fn to_sarif(card: &Scorecard) -> String {
    // `ignore` stays on the scorecard. Code scanning turns each SARIF result
    // into a pull-request annotation, so those findings are not uploaded.
    let findings: Vec<&Finding> = card
        .sections
        .findings
        .iter()
        .filter(|finding| finding.disposition != "ignore")
        .collect();
    let mut rules = Vec::new();
    let mut seen = Vec::new();
    for finding in &findings {
        if !seen.iter().any(|rule: &String| rule == &finding.rule) {
            seen.push(finding.rule.clone());
            rules.push(json!({
                "id": finding.rule,
                "shortDescription": {"text": finding.rule},
            }));
        }
    }
    let results: Vec<Value> = findings.iter().copied().map(result).collect();
    let gates: Vec<Value> = card
        .measures
        .gates
        .iter()
        .map(|gate| {
            json!({
                "id": gate.id,
                "pass": gate.pass,
                "enforced": gate.enforced,
            })
        })
        .collect();
    let doc = json!({
        "$schema": "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/master/Schemata/sarif-schema-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "sc",
                    "version": card.identity.version,
                    "informationUri": "https://github.com/moonbase2090/Scorecard",
                    "rules": rules,
                }
            },
            "results": results,
            "properties": {
                "scorecardVerdict": sc_core::report_verdict(card),
                "scorecardGates": gates,
            },
        }]
    });
    serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".into())
}

fn result(finding: &Finding) -> Value {
    // Code scanning turns every SARIF result into an annotation, and treats
    // `error` results as security alerts. Only the secrets engine reports
    // secrets; everything else (test failures, missing coverage, lint, CRAP)
    // is a quality signal, so it uploads at `warning` even when the finding
    // itself is `error` and fails a gate. The JSON report keeps the finding
    // severity; this mapping only affects the SARIF upload.
    let level = if finding.engine == "secrets" {
        if finding.severity == "error" {
            "error"
        } else if finding.severity == "warning" {
            "warning"
        } else {
            "note"
        }
    } else if finding.severity == "error" || finding.severity == "warning" {
        "warning"
    } else {
        "note"
    };
    let mut location = json!({
        "physicalLocation": {
            "artifactLocation": {"uri": finding.file},
        }
    });
    if let Some(span) = &finding.span {
        location["physicalLocation"]["region"] = json!({
            "startLine": span.start_line.max(1),
            "startColumn": span.start_col.max(1),
        });
    }
    json!({
        "ruleId": finding.rule,
        "level": level,
        "message": {"text": finding.message},
        "locations": [location],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sarif_names_the_rule() {
        let mut card = Scorecard::skeleton(".", 30);
        card.sections.findings.push(Finding {
            id: "secrets:src/lib.rs:1".into(),
            rule: "secrets.github_token".into(),
            engine: "secrets".into(),
            severity: "error".into(),
            file: "src/lib.rs".into(),
            span: None,
            symbol: None,
            message: "token".into(),
            evidence: serde_json::json!({}),
            suggested_action: None,
            disposition: String::new(),
        });
        let text = to_sarif(&card);
        assert!(text.contains("\"version\": \"2.1.0\""));
        assert!(text.contains("secrets.github_token"));
    }

    #[test]
    fn non_secret_errors_upload_as_warnings() {
        // `test.failed` is severity error and fails the tests gate, but it is
        // a quality signal, not a secret: code scanning must not treat it as
        // a security alert. The JSON finding keeps severity error.
        let mut card = Scorecard::skeleton(".", 30);
        card.sections.findings.push(Finding {
            id: "test:src/lib.rs:it_adds".into(),
            rule: "test.failed".into(),
            engine: "tests".into(),
            severity: "error".into(),
            file: "src/lib.rs".into(),
            span: None,
            symbol: Some("it_adds".into()),
            message: "test it_adds failed".into(),
            evidence: serde_json::json!({}),
            suggested_action: None,
            disposition: "fix".into(),
        });
        card.sections.findings.push(Finding {
            id: "coverage:missing".into(),
            rule: "coverage.missing".into(),
            engine: "coverage".into(),
            severity: "warning".into(),
            file: ".".into(),
            span: None,
            symbol: None,
            message: "coverage was not measured".into(),
            evidence: serde_json::json!({}),
            suggested_action: None,
            disposition: "ask".into(),
        });
        let text = to_sarif(&card);
        assert!(text.contains("test.failed"), "{text}");
        assert!(text.contains("coverage.missing"), "{text}");
        assert!(
            !text.contains("\"level\": \"error\"") && !text.contains("\"level\":\"error\""),
            "{text}"
        );
        // Secrets still upload at error so real leaks stay security alerts.
        let mut secrets = Scorecard::skeleton(".", 30);
        secrets.sections.findings.push(Finding {
            id: "secrets:src/lib.rs:1".into(),
            rule: "secrets.github_token".into(),
            engine: "secrets".into(),
            severity: "error".into(),
            file: "src/lib.rs".into(),
            span: None,
            symbol: None,
            message: "token".into(),
            evidence: serde_json::json!({}),
            suggested_action: None,
            disposition: "fix".into(),
        });
        let secrets_text = to_sarif(&secrets);
        assert!(
            secrets_text.contains("\"level\": \"error\"")
                || secrets_text.contains("\"level\":\"error\""),
            "{secrets_text}"
        );
    }

    #[test]
    fn sarif_includes_the_scorecard_verdict_and_gate_enforcement() {
        let mut card = Scorecard::skeleton(".", 30);
        card.verdict = "pass".into();
        card.measures.gates.push(sc_core::Gate {
            id: "lint".into(),
            pass: false,
            enforced: true,
            reason: Some("lint failed".into()),
        });
        let value: Value = serde_json::from_str(&to_sarif(&card)).unwrap();
        assert_eq!(value["runs"][0]["properties"]["scorecardVerdict"], "fail");
        assert_eq!(
            value["runs"][0]["properties"]["scorecardGates"][0]["enforced"],
            true
        );
    }

    #[test]
    fn an_ignored_perf_finding_is_not_uploaded() {
        let perf_rule = format!("perf.{}", "clone_in_loop");
        let mut card = Scorecard::skeleton(".", 30);
        card.sections.findings.push(Finding {
            id: format!("perf:tests/rows.rs:owned_rows:{perf_rule}"),
            rule: perf_rule.clone(),
            engine: "perf".into(),
            severity: "warning".into(),
            file: "tests/rows.rs".into(),
            span: None,
            symbol: Some("owned_rows".into()),
            message: "clone inside a loop in owned_rows".into(),
            evidence: serde_json::json!({}),
            suggested_action: None,
            disposition: String::new(),
        });
        card.sections.findings.push(Finding {
            id: "secrets:src/lib.rs:1".into(),
            rule: "secrets.github_token".into(),
            engine: "secrets".into(),
            severity: "error".into(),
            file: "src/lib.rs".into(),
            span: None,
            symbol: None,
            message: "token".into(),
            evidence: serde_json::json!({}),
            suggested_action: None,
            disposition: String::new(),
        });
        sc_core::apply_disposition(&mut card.sections.findings);
        assert_eq!(card.sections.findings[0].disposition, "ignore");
        assert_eq!(card.sections.findings[1].disposition, "fix");
        let text = to_sarif(&card);
        assert!(!text.contains("tests/rows.rs"), "{text}");
        assert!(!text.contains(&perf_rule), "{text}");
        assert!(text.contains("secrets.github_token"), "{text}");
        assert!(text.contains("src/lib.rs"), "{text}");
    }
}

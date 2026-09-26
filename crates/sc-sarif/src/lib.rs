// SPDX-License-Identifier: MPL-2.0
//! SARIF 2.1.0 export of a scorecard.

use sc_core::{Finding, Scorecard};
use serde_json::{json, Value};

pub fn to_sarif(card: &Scorecard) -> String {
    let mut rules = Vec::new();
    let mut seen = Vec::new();
    for finding in &card.findings {
        if !seen.iter().any(|rule: &String| rule == &finding.rule) {
            seen.push(finding.rule.clone());
            rules.push(json!({
                "id": finding.rule,
                "shortDescription": {"text": finding.rule},
            }));
        }
    }
    let results: Vec<Value> = card.findings.iter().map(result).collect();
    let doc = json!({
        "$schema": "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/master/Schemata/sarif-schema-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "sc",
                    "version": card.version,
                    "informationUri": "https://github.com/moonbase2090/Scorecard",
                    "rules": rules,
                }
            },
            "results": results,
        }]
    });
    serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".into())
}

fn result(finding: &Finding) -> Value {
    let level = if finding.severity == "error" {
        "error"
    } else if finding.severity == "warning" {
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
        card.findings.push(Finding {
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
}

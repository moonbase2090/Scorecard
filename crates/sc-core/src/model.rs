// SPDX-License-Identifier: MPL-2.0
//! JSON contract. Field names are part of the stable scorecard schema.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::SCORECARD_VERSION;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub rule: String,
    pub engine: String,
    pub severity: String,
    pub file: String,
    pub span: Option<Span>,
    pub symbol: Option<String>,
    pub message: String,
    pub evidence: Value,
    pub suggested_action: Option<String>,
    /// `fix`, `ask`, or `ignore`. Empty until `apply_disposition`.
    #[serde(default)]
    pub disposition: String,
}

/// How an agent should treat a finding.
///
/// Errors from deterministic engines are `fix`. Missing tools and coverage
/// gaps are `ask`. Perf notes are `ignore`.
pub fn disposition_for(rule: &str, severity: &str) -> &'static str {
    match rule {
        "engine.unavailable"
        | "coverage.missing"
        | "coverage.unmatched"
        | "spec.llm_gap"
        | "sca.hallucinated_import" => "ask",
        "perf.nested_loop" | "perf.clone_in_loop" => "ignore",
        _ if severity == "warning" => "ask",
        _ => "fix",
    }
}

pub fn apply_disposition(findings: &mut [Finding]) {
    for finding in findings {
        finding.disposition = disposition_for(&finding.rule, &finding.severity).to_string();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitInfo {
    pub head: Option<String>,
    pub dirty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    pub mode: String,
    pub paths: Vec<String>,
    /// Resolved git base of a diff-scoped run (`--diff`). Absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scores {
    pub correctness: f64,
    pub efficiency: f64,
    pub maintainability: f64,
    pub security: f64,
    /// Accessibility. Missing from older scorecards, which score as 1.0.
    #[serde(default = "one_score")]
    pub a11y: f64,
}

fn one_score() -> f64 {
    1.0
}

impl Scores {
    pub fn perfect() -> Self {
        Self {
            correctness: 1.0,
            efficiency: 1.0,
            maintainability: 1.0,
            security: 1.0,
            a11y: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gate {
    pub id: String,
    pub pass: bool,
    /// When false, the gate is reported and does not fail the process.
    #[serde(default = "enforced_gate")]
    pub enforced: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

fn enforced_gate() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    pub loc_changed: u64,
    pub files_changed: u64,
    pub coverage_changed: f64,
    pub crap_max: f64,
    pub crap_over_threshold: u64,
    pub hallucinated_imports: u64,
}

impl Metrics {
    pub fn zeros() -> Self {
        Self {
            loc_changed: 0,
            files_changed: 0,
            coverage_changed: 0.0,
            crap_max: 0.0,
            crap_over_threshold: 0,
            hallucinated_imports: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CrapFunction {
    pub symbol: String,
    pub file: String,
    pub cc: u32,
    pub coverage: f64,
    pub crap: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CrapSection {
    pub threshold: u32,
    pub worst: Vec<CrapFunction>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MutationSection {
    pub status: String,
    pub score: Option<f64>,
    pub killed: u64,
    pub survived: u64,
    pub timeout: u64,
}

impl MutationSection {
    pub fn skipped() -> Self {
        Self {
            status: "skipped".to_string(),
            score: None,
            killed: 0,
            survived: 0,
            timeout: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpecSection {
    pub path: Option<String>,
    pub gaps: Vec<Value>,
    /// Chat rounds the spec-gap model used, when `--llm` ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_rounds: Option<u32>,
}

impl SpecSection {
    pub fn empty() -> Self {
        Self {
            path: None,
            gaps: Vec::new(),
            llm_rounds: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRecord {
    pub engine: String,
    pub command: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    /// Remaining analysis budget when this run started, if tracked by the engine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scorecard {
    pub version: String,
    pub id: String,
    pub repo: String,
    /// `rust`, `node`, `python`, `bash`, `go`, `java`, `csharp`, `php`, `cpp`, `command`, `unknown`, or `ambiguous`.
    #[serde(default)]
    pub pack: String,
    /// `rust-tests` selects Rust `#[test]` names. Every other pack uses `full-suite`.
    #[serde(default)]
    pub test_selection: String,
    pub git: GitInfo,
    /// Caller-supplied goal. Not inferred from transcripts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    pub scope: Scope,
    pub verdict: String,
    pub engines_run: Vec<String>,
    pub engines_skipped: Vec<String>,
    pub scores: Scores,
    pub gates: Vec<Gate>,
    pub metrics: Metrics,
    pub crap: CrapSection,
    pub mutation: MutationSection,
    pub findings: Vec<Finding>,
    pub spec: SpecSection,
    #[serde(default)]
    pub runs: Vec<RunRecord>,
}

impl Scorecard {
    pub fn skeleton(repo: impl Into<String>, threshold: u32) -> Self {
        Self {
            version: SCORECARD_VERSION.to_string(),
            id: crate::new_scorecard_id(),
            repo: repo.into(),
            pack: "unknown".into(),
            test_selection: "full-suite".into(),
            git: GitInfo {
                head: None,
                dirty: false,
            },
            scope: Scope {
                mode: "tree".to_string(),
                paths: Vec::new(),
                base: None,
            },
            intent: None,
            verdict: "fail".to_string(),
            engines_run: Vec::new(),
            engines_skipped: vec![
                "compile".into(),
                "tests".into(),
                "coverage".into(),
                "complexity".into(),
                "crap".into(),
                "llm".into(),
                "mutation".into(),
            ],
            scores: Scores::perfect(),
            gates: Vec::new(),
            metrics: Metrics::zeros(),
            crap: CrapSection {
                threshold,
                worst: Vec::new(),
            },
            mutation: MutationSection::skipped(),
            findings: Vec::new(),
            spec: SpecSection::empty(),
            runs: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scope_base_is_optional_and_omitted_when_absent() {
        let mut card = Scorecard::skeleton("demo", 30);
        let json = serde_json::to_value(&card).unwrap();
        assert!(json["scope"].get("base").is_none());
        card.scope.mode = "diff".into();
        card.scope.base = Some("origin/main".into());
        let json = serde_json::to_value(&card).unwrap();
        assert_eq!(json["scope"]["base"], "origin/main");
        // Scorecards written before the field existed still load.
        let old: Scope = serde_json::from_str(r#"{"mode":"tree","paths":[]}"#).unwrap();
        assert_eq!(old.base, None);
    }

    #[test]
    fn field_names_match_the_contract() {
        let card = Scorecard {
            version: "0.1".into(),
            id: "abc".into(),
            repo: ".".into(),
            pack: "rust".into(),
            test_selection: "full-suite".into(),
            git: GitInfo {
                head: Some("abc123".into()),
                dirty: true,
            },
            scope: Scope {
                mode: "tree".into(),
                paths: vec!["src/parse.rs".into()],
                base: None,
            },
            intent: Some("keep parse_input under the CRAP threshold".into()),
            verdict: "fail".into(),
            engines_run: vec!["compile".into(), "tests".into()],
            engines_skipped: vec!["llm".into(), "mutation".into()],
            scores: Scores {
                correctness: 0.62,
                efficiency: 0.81,
                maintainability: 0.54,
                security: 0.9,
                a11y: 1.0,
            },
            gates: vec![
                Gate {
                    id: "types".into(),
                    pass: true,
                    enforced: true,
                    reason: None,
                },
                Gate {
                    id: "crap".into(),
                    pass: false,
                    enforced: true,
                    reason: Some("4 functions over threshold".into()),
                },
            ],
            metrics: Metrics {
                loc_changed: 1840,
                files_changed: 12,
                coverage_changed: 0.64,
                crap_max: 156.0,
                crap_over_threshold: 4,
                hallucinated_imports: 1,
            },
            crap: CrapSection {
                threshold: 30,
                worst: vec![CrapFunction {
                    symbol: "parse_input".into(),
                    file: "src/parse.rs".into(),
                    cc: 12,
                    coverage: 0.0,
                    crap: 156.0,
                }],
            },
            mutation: MutationSection::skipped(),
            findings: vec![Finding {
                id: "crap:src/parse.rs:parse_input".into(),
                rule: "crap.over_threshold".into(),
                engine: "crap".into(),
                severity: "error".into(),
                file: "src/parse.rs".into(),
                span: Some(Span {
                    start_line: 42,
                    start_col: 1,
                    end_line: 88,
                    end_col: 2,
                }),
                symbol: Some("parse_input".into()),
                message: "CRAP 156 (CC=12, cov=0%) exceeds threshold 30".into(),
                evidence: json!({"cc": 12, "coverage": 0.0, "crap": 156.0}),
                suggested_action: Some("Add tests covering branches or split the function".into()),
                disposition: String::new(),
            }],
            spec: SpecSection {
                path: Some("TASK.md".into()),
                gaps: vec![],
                llm_rounds: None,
            },
            runs: vec![RunRecord {
                engine: "tests".into(),
                command: "cargo test".into(),
                exit_code: Some(0),
                duration_ms: 12,
                budget_ms: None,
            }],
        };

        let v = serde_json::to_value(&card).unwrap();
        for key in [
            "version",
            "id",
            "repo",
            "pack",
            "test_selection",
            "git",
            "scope",
            "verdict",
            "engines_run",
            "engines_skipped",
            "scores",
            "gates",
            "metrics",
            "crap",
            "mutation",
            "findings",
            "spec",
            "intent",
            "runs",
        ] {
            assert!(v.get(key).is_some(), "missing scorecard.{key}");
        }
        assert!(v["gates"][0].get("reason").is_none());
        assert_eq!(v["gates"][1]["reason"], "4 functions over threshold");
        assert!(v["mutation"]["score"].is_null());

        let finding = &v["findings"][0];
        for key in [
            "id",
            "rule",
            "engine",
            "severity",
            "file",
            "span",
            "symbol",
            "message",
            "evidence",
            "suggested_action",
            "disposition",
        ] {
            assert!(finding.get(key).is_some(), "missing finding.{key}");
        }
        assert_eq!(disposition_for("crap.over_threshold", "error"), "fix");
        assert_eq!(disposition_for("engine.unavailable", "warning"), "ask");
        assert_eq!(disposition_for("perf.nested_loop", "warning"), "ignore");
        assert_eq!(disposition_for("sca.hallucinated_import", "warning"), "ask");
        assert_eq!(finding["span"]["start_line"], 42);
        assert_eq!(finding["evidence"]["cc"], 12);
        assert_eq!(finding["evidence"]["crap"], 156.0);
    }
}

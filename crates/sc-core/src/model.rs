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
/// gaps are `ask`. A performance hint (`perf.*`) is `ignore`.
pub fn disposition_for(rule: &str, severity: &str) -> &'static str {
    match rule {
        "engine.unavailable"
        | "coverage.missing"
        | "coverage.unmatched"
        | "spec.llm_gap"
        | "sca.hallucinated_import" => "ask",
        _ if rule.starts_with("perf.") => "ignore",
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
    /// Changed paths, relative to the analyzed directory. Omitted when clean.
    /// Older scorecards without this field still deserialize.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dirty_paths: Vec<String>,
}

impl GitInfo {
    pub fn status_label(&self) -> String {
        if self.dirty { "dirty" } else { "clean" }.into()
    }

    /// A compact list for human-readable reports. JSON retains every path.
    pub fn changed_paths_label(&self) -> String {
        let mut label = String::new();
        for (index, path) in self.dirty_paths.iter().take(3).enumerate() {
            if index > 0 {
                label.push_str(", ");
            }
            for ch in path.chars() {
                label.extend(ch.escape_default());
            }
        }
        let remaining = self.dirty_paths.len().saturating_sub(3);
        if remaining > 0 {
            if !label.is_empty() {
                label.push_str(", ");
            }
            label.push_str(&format!("+{remaining} more"));
        }
        label
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    pub mode: String,
    pub paths: Vec<String>,
    /// Resolved git base of a diff-scoped run (`--diff`). Absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// Source paths in the tree that are not in this `--diff` selection.
    /// Absent outside diff mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub other_paths: Option<u64>,
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
    /// When false, the gate does not affect the report verdict or exit code.
    /// When true, a failure affects the report verdict; `--fail-on` separately
    /// selects which enforced failures set exit 1.
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
    /// Names that are not local, not installed, and not on the package index.
    pub hallucinated_imports: u64,
    /// Installed or published packages that are imported and not declared.
    #[serde(default)]
    pub undeclared_dependencies: u64,
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
            undeclared_dependencies: 0,
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

/// LLM review summary. Omitted when `--llm` is off. Old scorecards load without it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmSection {
    /// `ran` or `skipped`.
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rounds: Option<u32>,
    /// `gaps found` or `no gaps` when the review ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// Plain reason when `status` is `skipped`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl LlmSection {
    pub fn skipped(reason: impl Into<String>) -> Self {
        Self {
            status: "skipped".into(),
            backend: None,
            model: None,
            rounds: None,
            verdict: None,
            notes: Vec::new(),
            reason: Some(reason.into()),
        }
    }

    pub fn ran(
        backend: impl Into<String>,
        model: impl Into<String>,
        rounds: u32,
        gaps: usize,
        notes: Vec<String>,
    ) -> Self {
        Self {
            status: "ran".into(),
            backend: Some(backend.into()),
            model: Some(model.into()),
            rounds: Some(rounds),
            verdict: Some(if gaps == 0 { "no gaps" } else { "gaps found" }.into()),
            notes,
            reason: None,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedFilesWarning {
    pub files: Vec<String>,
    pub suggested_action: String,
}

impl GeneratedFilesWarning {
    pub fn from_paths(files: Vec<String>) -> Option<Self> {
        if files.is_empty() {
            return None;
        }
        Some(Self {
            files,
            suggested_action: "Add these paths to scope.exclude in analyzer.toml to skip them."
                .into(),
        })
    }
}

/// Report identity. Flattened into the scorecard JSON object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScorecardIdentity {
    pub version: String,
    pub id: String,
    pub repo: String,
    /// `rust`, `node`, `python`, `bash`, `go`, `java`, `csharp`, `php`, `cpp`, `command`, `unknown`, or `ambiguous`.
    #[serde(default)]
    pub pack: String,
    /// `rust-tests` selects Rust `#[test]` names. Every other pack uses `full-suite`.
    #[serde(default)]
    pub test_selection: String,
}

/// Git, caller intent, and the paths this run scored. Flattened into the scorecard JSON object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScorecardContext {
    pub git: GitInfo,
    /// Caller-supplied goal. Not inferred from transcripts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    pub scope: Scope,
}

/// Engines that ran and engines that were skipped. Flattened into the scorecard JSON object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScorecardEngines {
    pub engines_run: Vec<String>,
    pub engines_skipped: Vec<String>,
}

/// Scores, gates, and diff metrics. Flattened into the scorecard JSON object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScorecardMeasures {
    pub scores: Scores,
    pub gates: Vec<Gate>,
    pub metrics: Metrics,
}

/// Findings and the per-engine sections. Flattened into the scorecard JSON object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScorecardSections {
    pub crap: CrapSection,
    pub mutation: MutationSection,
    pub findings: Vec<Finding>,
    /// Generated or vendored source files included in this scan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_files_warning: Option<GeneratedFilesWarning>,
    pub spec: SpecSection,
    /// Set when `--llm on` ran, or when it was turned on and then skipped.
    /// Omitted when llm is off. Old scorecards load without this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm: Option<LlmSection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scorecard {
    #[serde(flatten)]
    pub identity: ScorecardIdentity,
    #[serde(flatten)]
    pub context: ScorecardContext,
    pub verdict: String,
    #[serde(flatten)]
    pub engines: ScorecardEngines,
    #[serde(flatten)]
    pub measures: ScorecardMeasures,
    #[serde(flatten)]
    pub sections: ScorecardSections,
    #[serde(default)]
    pub runs: Vec<RunRecord>,
}

impl Scorecard {
    pub fn skeleton(repo: impl Into<String>, threshold: u32) -> Self {
        Self {
            identity: ScorecardIdentity {
                version: SCORECARD_VERSION.to_string(),
                id: crate::new_scorecard_id(),
                repo: repo.into(),
                pack: "unknown".into(),
                test_selection: "full-suite".into(),
            },
            context: ScorecardContext {
                git: GitInfo {
                    head: None,
                    dirty: false,
                    dirty_paths: Vec::new(),
                },
                intent: None,
                scope: Scope {
                    mode: "tree".to_string(),
                    paths: Vec::new(),
                    base: None,
                    other_paths: None,
                },
            },
            verdict: "fail".to_string(),
            engines: ScorecardEngines {
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
            },
            measures: ScorecardMeasures {
                scores: Scores::perfect(),
                gates: Vec::new(),
                metrics: Metrics::zeros(),
            },
            sections: ScorecardSections {
                crap: CrapSection {
                    threshold,
                    worst: Vec::new(),
                },
                mutation: MutationSection::skipped(),
                findings: Vec::new(),
                generated_files_warning: None,
                spec: SpecSection::empty(),
                llm: None,
            },
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
        card.context.scope.mode = "diff".into();
        card.context.scope.base = Some("origin/main".into());
        let json = serde_json::to_value(&card).unwrap();
        assert_eq!(json["scope"]["base"], "origin/main");
        // Scorecards written before the field existed still load.
        let old: Scope = serde_json::from_str(r#"{"mode":"tree","paths":[]}"#).unwrap();
        assert_eq!(old.base, None);
        assert_eq!(old.other_paths, None);
        card.context.scope.other_paths = Some(40);
        let json = serde_json::to_value(&card).unwrap();
        assert_eq!(json["scope"]["other_paths"], 40);
    }

    #[test]
    fn scope_other_paths_omitted_when_absent() {
        let card = Scorecard::skeleton("demo", 30);
        let json = serde_json::to_value(&card).unwrap();
        assert!(json["scope"].get("other_paths").is_none());
    }

    #[test]
    fn llm_section_is_optional_and_round_trips() {
        let mut card = Scorecard::skeleton("demo", 30);
        let json = serde_json::to_value(&card).unwrap();
        assert!(json.get("llm").is_none());
        let old: Scorecard = serde_json::from_value(json).unwrap();
        assert!(old.sections.llm.is_none());
        card.sections.llm = Some(LlmSection::ran(
            "ollama",
            "qwen2.5-coder",
            4,
            0,
            vec!["checked src/lib.rs".into()],
        ));
        let json = serde_json::to_value(&card).unwrap();
        assert_eq!(json["llm"]["verdict"], "no gaps");
        assert_eq!(json["llm"]["notes"][0], "checked src/lib.rs");
        let back: Scorecard = serde_json::from_value(json).unwrap();
        assert_eq!(back.sections.llm.unwrap().status, "ran");
    }

    #[test]
    fn field_names_match_the_contract() {
        let card = Scorecard {
            identity: ScorecardIdentity {
                version: "0.1".into(),
                id: "abc".into(),
                repo: ".".into(),
                pack: "rust".into(),
                test_selection: "full-suite".into(),
            },
            context: ScorecardContext {
                git: GitInfo {
                    head: Some("abc123".into()),
                    dirty: true,
                    dirty_paths: Vec::new(),
                },
                intent: Some("keep parse_input under the CRAP threshold".into()),
                scope: Scope {
                    mode: "tree".into(),
                    paths: vec!["src/parse.rs".into()],
                    base: None,
                    other_paths: None,
                },
            },
            verdict: "fail".into(),
            engines: ScorecardEngines {
                engines_run: vec!["compile".into(), "tests".into()],
                engines_skipped: vec!["llm".into(), "mutation".into()],
            },
            measures: ScorecardMeasures {
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
                    undeclared_dependencies: 2,
                },
            },
            sections: ScorecardSections {
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
                    suggested_action: Some(
                        "Add tests covering branches or split the function".into(),
                    ),
                    disposition: String::new(),
                }],
                generated_files_warning: None,
                spec: SpecSection {
                    path: Some("TASK.md".into()),
                    gaps: vec![],
                    llm_rounds: None,
                },
                llm: None,
            },
            runs: vec![RunRecord {
                engine: "tests".into(),
                command: "cargo test".into(),
                exit_code: Some(0),
                duration_ms: 12,
                budget_ms: None,
            }],
        };

        assert_eq!(
            serde_json::to_string(&card).unwrap(),
            include_str!("golden/scorecard-contract.json"),
            "compact scorecard JSON changed"
        );
        assert_eq!(
            serde_json::to_string_pretty(&card).unwrap(),
            include_str!("golden/scorecard-contract.pretty.json"),
            "pretty scorecard JSON changed"
        );
        let back: Scorecard =
            serde_json::from_str(include_str!("golden/scorecard-contract.json")).unwrap();
        assert_eq!(
            serde_json::to_string(&back).unwrap(),
            include_str!("golden/scorecard-contract.json"),
            "reloaded scorecard JSON changed"
        );

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
        assert_eq!(disposition_for("sca.hallucinated_import", "warning"), "ask");
        let perf_rule = format!("perf.{}", "clone_in_loop");
        assert_eq!(disposition_for(&perf_rule, "warning"), "ignore");
        assert_eq!(finding["span"]["start_line"], 42);
        assert_eq!(finding["evidence"]["cc"], 12);
        assert_eq!(finding["evidence"]["crap"], 156.0);
    }
}

// SPDX-License-Identifier: MIT
//! Baseline gating: a failing gate sets exit 1 only when it fails on a finding
//! that is not in the stored baseline.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{Finding, Scorecard};

pub const BASELINE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Baseline {
    pub version: u32,
    /// `Finding::fingerprint` values for every finding present when the baseline was written.
    pub fingerprints: BTreeSet<String>,
}

impl Baseline {
    pub fn from_findings(findings: &[Finding]) -> Self {
        Self {
            version: BASELINE_VERSION,
            fingerprints: findings.iter().map(Finding::fingerprint).collect(),
        }
    }

    pub fn contains(&self, finding: &Finding) -> bool {
        self.fingerprints.contains(&finding.fingerprint())
    }
}

/// The engine whose findings explain a gate. Every gate id names its engine,
/// except `types`, which the `compile` engine reports.
fn engine_for_gate(gate: &str) -> &str {
    if gate == "types" {
        "compile"
    } else {
        gate
    }
}

/// Enforced gates that failed for a reason the baseline does not cover.
///
/// A failing gate is covered when every finding from its engine is in the
/// baseline. A failing gate with no findings from its engine is never covered,
/// because nothing in the baseline can explain it.
pub fn gates_failing_on_new_findings(card: &Scorecard, baseline: &Baseline) -> Vec<String> {
    card.measures
        .gates
        .iter()
        .filter(|gate| gate.enforced && !gate.pass)
        .filter(|gate| {
            let engine = engine_for_gate(&gate.id);
            let mut own = card
                .sections
                .findings
                .iter()
                .filter(|finding| finding.engine == engine)
                .peekable();
            own.peek().is_none() || own.any(|finding| !baseline.contains(finding))
        })
        .map(|gate| gate.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{Gate, Scorecard};

    fn finding(engine: &str, rule: &str, file: &str, message: &str) -> Finding {
        Finding {
            id: format!("{engine}-{rule}-{file}"),
            rule: rule.into(),
            engine: engine.into(),
            severity: "error".into(),
            file: file.into(),
            span: None,
            symbol: None,
            message: message.into(),
            evidence: json!({}),
            suggested_action: None,
            disposition: String::new(),
        }
    }

    fn gate(id: &str, pass: bool, enforced: bool) -> Gate {
        Gate {
            id: id.into(),
            pass,
            enforced,
            reason: None,
        }
    }

    fn card(gates: Vec<Gate>, findings: Vec<Finding>) -> Scorecard {
        let mut card = Scorecard::skeleton("repo", 30);
        card.measures.gates = gates;
        card.sections.findings = findings;
        card
    }

    #[test]
    fn known_findings_do_not_fail_the_gate() {
        let known = finding(
            "crap",
            "crap.over_threshold",
            "src/lib.rs",
            "add is over 30",
        );
        let baseline = Baseline::from_findings(std::slice::from_ref(&known));
        let card = card(vec![gate("crap", false, true)], vec![known]);
        assert_eq!(
            gates_failing_on_new_findings(&card, &baseline),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_new_finding_fails_its_gate_and_only_its_gate() {
        let known = finding(
            "crap",
            "crap.over_threshold",
            "src/lib.rs",
            "add is over 30",
        );
        let fresh = finding("secrets", "secrets.aws_access_key", "src/key.rs", "AWS key");
        let baseline = Baseline::from_findings(std::slice::from_ref(&known));
        let card = card(
            vec![gate("crap", false, true), gate("secrets", false, true)],
            vec![known, fresh],
        );
        assert_eq!(gates_failing_on_new_findings(&card, &baseline), ["secrets"]);
    }

    #[test]
    fn a_failing_gate_with_no_findings_is_never_covered() {
        let baseline = Baseline::from_findings(&[]);
        let card = card(vec![gate("tests", false, true)], vec![]);
        assert_eq!(gates_failing_on_new_findings(&card, &baseline), ["tests"]);
    }

    #[test]
    fn types_gate_is_explained_by_compile_findings() {
        let error = finding("compile", "types.error", "src/lib.rs", "mismatched types");
        let baseline = Baseline::from_findings(std::slice::from_ref(&error));
        let known = card(vec![gate("types", false, true)], vec![error.clone()]);
        assert!(gates_failing_on_new_findings(&known, &baseline).is_empty());
        let unknown = card(vec![gate("types", false, true)], vec![]);
        assert_eq!(
            gates_failing_on_new_findings(&unknown, &baseline),
            ["types"]
        );
    }

    #[test]
    fn passing_and_unenforced_gates_never_count() {
        let baseline = Baseline::from_findings(&[]);
        let card = card(
            vec![gate("crap", true, true), gate("a11y", false, false)],
            vec![finding("a11y", "a11y.label", "x.html", "missing label")],
        );
        assert!(gates_failing_on_new_findings(&card, &baseline).is_empty());
    }

    #[test]
    fn baseline_round_trips_through_json() {
        let findings = [finding("crap", "crap.over_threshold", "src/lib.rs", "add")];
        let baseline = Baseline::from_findings(&findings);
        let text = serde_json::to_string(&baseline).unwrap();
        let back: Baseline = serde_json::from_str(&text).unwrap();
        assert_eq!(back, baseline);
        assert!(back.contains(&findings[0]));
        assert_eq!(back.version, BASELINE_VERSION);
    }
}

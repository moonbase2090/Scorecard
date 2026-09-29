// SPDX-License-Identifier: MPL-2.0
//! Dimension scores and the CRAP formula.
//!
//! Dimension scores are in `0.0..=1.0`. Each score starts at 1.0. Most
//! warnings subtract 0.05 and errors subtract 0.25, then the result is
//! clamped to 0.0.
//!
//! - correctness: `compile`, `tests`, `config`, `lint`, and `html` findings
//! - maintainability: `complexity`, `crap`, and `coverage` findings
//! - efficiency: `perf` findings (only when `engines.perf` is on)
//! - security: `secrets` and `sca` findings (`sca` warnings subtract 0.01)
//! - a11y: `a11y` findings

use crate::{Finding, Gate, Scores};

pub fn compute_scores(findings: &[Finding]) -> Scores {
    let mut correctness: f64 = 1.0;
    let mut efficiency: f64 = 1.0;
    let mut maintainability: f64 = 1.0;
    let mut security: f64 = 1.0;
    let mut a11y: f64 = 1.0;

    for finding in findings {
        let penalty = penalty_for(finding);
        if penalty == 0.0 {
            continue;
        }
        let slot = match finding.engine.as_str() {
            "compile" | "tests" | "config" | "lint" | "html" => &mut correctness,
            "perf" => &mut efficiency,
            "secrets" | "sca" => &mut security,
            "a11y" => &mut a11y,
            _ => &mut maintainability,
        };
        *slot -= penalty;
    }

    Scores {
        correctness: correctness.max(0.0),
        efficiency: efficiency.max(0.0),
        maintainability: maintainability.max(0.0),
        security: security.max(0.0),
        a11y: a11y.max(0.0),
    }
}

fn penalty_for(finding: &Finding) -> f64 {
    match (finding.engine.as_str(), finding.severity.as_str()) {
        // Undeclared dependencies are advisory (`sca` gate is never enforced by
        // default), so they weigh in proportionally instead of drowning out the
        // dimension score.
        ("sca", "warning") => 0.01,
        (_, "error") => 0.25,
        (_, "warning") => 0.05,
        _ => 0.0,
    }
}

/// `CRAP(m) = CC(m)^2 * (1 - cov(m))^3 + CC(m)`.
///
/// `coverage` is function line coverage in `0..=1` and is clamped.
/// A result equal to the threshold does not exceed it.
pub fn crap_score(cc: u32, coverage: f64) -> f64 {
    let cov = coverage.clamp(0.0, 1.0);
    let cc = f64::from(cc);
    cc * cc * (1.0 - cov).powi(3) + cc
}

pub fn exceeds_threshold(crap: f64, threshold: u32) -> bool {
    crap > f64::from(threshold)
}

pub fn verdict_fails(gates: &[Gate], fail_on: &[String]) -> bool {
    gates
        .iter()
        .any(|gate| gate.enforced && !gate.pass && fail_on.iter().any(|id| id == &gate.id))
}

/// Reconcile each gate's `enforced` flag with the run's `--fail-on` set.
///
/// A gate only fails the run when it is both statically enforced and named
/// in `fail_on`. Folding the set into the flag keeps every renderer (HTML,
/// terminal, markdown, JSON) honest: a failing gate outside `--fail-on`
/// reports as "reported only" instead of claiming "enforced: yes" on a
/// passing verdict. Verdict behavior is unchanged; `verdict_fails` already
/// requires both conditions.
pub fn apply_fail_on(gates: &mut [Gate], fail_on: &[String]) {
    for gate in gates {
        gate.enforced = gate.enforced && fail_on.iter().any(|id| id == &gate.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crap_matches_the_published_examples() {
        assert!((crap_score(12, 0.0) - 156.0).abs() < 1e-9);
        assert!((crap_score(5, 0.0) - 30.0).abs() < 1e-9);
        assert!((crap_score(6, 0.0) - 42.0).abs() < 1e-9);
        assert!((crap_score(10, 0.0) - 110.0).abs() < 1e-9);
        assert!((crap_score(10, 1.0) - 10.0).abs() < 1e-9);
        assert!(!exceeds_threshold(crap_score(5, 0.0), 30));
        assert!(exceeds_threshold(crap_score(6, 0.0), 30));
    }

    #[test]
    fn unenforced_gates_do_not_fail_the_verdict() {
        let gates = vec![Gate {
            id: "crap".into(),
            pass: false,
            enforced: false,
            reason: Some("not provided by this pack".into()),
        }];
        assert!(!verdict_fails(&gates, &["crap".into()]));
    }

    fn failing_gate(id: &str) -> Gate {
        Gate {
            id: id.into(),
            pass: false,
            enforced: true,
            reason: Some("over threshold".into()),
        }
    }

    #[test]
    fn apply_fail_on_clears_enforced_outside_the_set() {
        // Dogfood case: --fail-on "" with a failing crap gate. The verdict
        // passes, so the gate must report as reported-only, not enforced.
        let mut gates = vec![failing_gate("crap"), failing_gate("tests")];
        apply_fail_on(&mut gates, &[]);
        assert!(gates.iter().all(|gate| !gate.enforced));
        assert!(!verdict_fails(&gates, &[]));
    }

    #[test]
    fn apply_fail_on_keeps_enforced_inside_the_set() {
        let mut gates = vec![failing_gate("crap"), failing_gate("tests")];
        apply_fail_on(&mut gates, &["crap".into()]);
        assert!(gates[0].enforced);
        assert!(!gates[1].enforced);
        assert!(verdict_fails(&gates, &["crap".into()]));
    }

    #[test]
    fn scores_subtract_from_one_and_floor_at_zero() {
        let findings = vec![
            finding("compile", "error"),
            finding("tests", "error"),
            finding("crap", "error"),
            finding("coverage", "warning"),
        ];
        let scores = compute_scores(&findings);
        assert!((scores.correctness - 0.5).abs() < 1e-9);
        assert!((scores.maintainability - 0.7).abs() < 1e-9);
        assert!((scores.efficiency - 1.0).abs() < 1e-9);
        assert!((scores.security - 1.0).abs() < 1e-9);

        let many: Vec<_> = (0..10).map(|_| finding("compile", "error")).collect();
        assert_eq!(compute_scores(&many).correctness, 0.0);
    }

    #[test]
    fn advisory_sca_warnings_have_a_smaller_security_penalty() {
        let findings: Vec<_> = (0..16).map(|_| finding("sca", "warning")).collect();
        let scores = compute_scores(&findings);
        assert!((scores.security - 0.84).abs() < 1e-9, "{scores:?}");
    }

    fn finding(engine: &str, severity: &str) -> Finding {
        Finding {
            id: "x".into(),
            rule: "r".into(),
            engine: engine.into(),
            severity: severity.into(),
            file: ".".into(),
            span: None,
            symbol: None,
            message: "m".into(),
            evidence: serde_json::json!({}),
            suggested_action: None,
            disposition: String::new(),
        }
    }
}

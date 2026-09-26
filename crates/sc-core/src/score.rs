// SPDX-License-Identifier: MPL-2.0
//! Dimension scores and the CRAP formula.
//!
//! Dimension scores are in `0.0..=1.0`. Each score starts at 1.0. An error
//! finding subtracts 0.25 and a warning subtracts 0.05, then the result is
//! clamped to 0.0.
//!
//! - correctness: `compile`, `tests`, and `config` findings
//! - maintainability: `complexity`, `crap`, and `coverage` findings
//! - efficiency: `perf` findings (none in M1, so this stays 1.0)
//! - security: `secrets` and `sca` findings (none in M1, so this stays 1.0)

use crate::{Finding, Gate, Scores};

pub fn compute_scores(findings: &[Finding]) -> Scores {
    let mut correctness: f64 = 1.0;
    let mut efficiency: f64 = 1.0;
    let mut maintainability: f64 = 1.0;
    let mut security: f64 = 1.0;

    for finding in findings {
        let penalty = match finding.severity.as_str() {
            "error" => 0.25,
            "warning" => 0.05,
            _ => 0.0,
        };
        if penalty == 0.0 {
            continue;
        }
        let slot = match finding.engine.as_str() {
            "compile" | "tests" | "config" | "lint" => &mut correctness,
            "perf" => &mut efficiency,
            "secrets" | "sca" => &mut security,
            _ => &mut maintainability,
        };
        *slot -= penalty;
    }

    Scores {
        correctness: correctness.max(0.0),
        efficiency: efficiency.max(0.0),
        maintainability: maintainability.max(0.0),
        security: security.max(0.0),
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

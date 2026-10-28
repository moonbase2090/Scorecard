// SPDX-License-Identifier: MIT
//! Changes between two runs of the same repository.

use std::collections::BTreeSet;

use crate::{Finding, Scorecard};

#[derive(Debug, Clone, PartialEq)]
pub struct Trend {
    /// `(name, previous, current)` for each score that changed.
    pub scores: Vec<(&'static str, f64, f64)>,
    /// Distinct finding fingerprints present now and not before.
    pub new_findings: usize,
    /// Distinct finding fingerprints present before and not now.
    pub resolved_findings: usize,
}

/// Compare two runs. `None` when they are not of the same repository, since a
/// delta between different projects would mean nothing.
pub fn trend(previous: &Scorecard, current: &Scorecard) -> Option<Trend> {
    if previous.identity.repo != current.identity.repo {
        return None;
    }
    let scores = score_pairs(previous)
        .into_iter()
        .zip(score_pairs(current))
        .filter(|((_, before), (_, after))| (before - after).abs() > f64::EPSILON)
        .map(|((name, before), (_, after))| (name, before, after))
        .collect();
    let before = fingerprints(previous);
    let after = fingerprints(current);
    Some(Trend {
        scores,
        new_findings: after.difference(&before).count(),
        resolved_findings: before.difference(&after).count(),
    })
}

fn score_pairs(card: &Scorecard) -> [(&'static str, f64); 5] {
    let scores = &card.measures.scores;
    [
        ("correctness", scores.correctness),
        ("efficiency", scores.efficiency),
        ("maintainability", scores.maintainability),
        ("security", scores.security),
        ("a11y", scores.a11y),
    ]
}

fn fingerprints(card: &Scorecard) -> BTreeSet<String> {
    card.sections
        .findings
        .iter()
        .map(Finding::fingerprint)
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn finding(message: &str) -> Finding {
        Finding {
            id: message.into(),
            rule: "crap.over_threshold".into(),
            engine: "crap".into(),
            severity: "error".into(),
            file: "src/lib.rs".into(),
            span: None,
            symbol: None,
            message: message.into(),
            evidence: json!({}),
            suggested_action: None,
            disposition: String::new(),
        }
    }

    fn card(repo: &str, maintainability: f64, findings: &[&str]) -> Scorecard {
        let mut card = Scorecard::skeleton(repo, 30);
        card.measures.scores.maintainability = maintainability;
        card.sections.findings = findings.iter().map(|m| finding(m)).collect();
        card
    }

    #[test]
    fn reports_changed_scores_and_finding_movement() {
        let before = card("repo", 0.9, &["a", "b"]);
        let after = card("repo", 0.7, &["b", "c"]);
        let t = trend(&before, &after).unwrap();
        assert_eq!(t.scores, vec![("maintainability", 0.9, 0.7)]);
        assert_eq!(t.new_findings, 1);
        assert_eq!(t.resolved_findings, 1);
    }

    #[test]
    fn unchanged_runs_have_no_score_change_and_no_movement() {
        let before = card("repo", 0.9, &["a"]);
        let t = trend(&before, &before.clone()).unwrap();
        assert!(t.scores.is_empty());
        assert_eq!((t.new_findings, t.resolved_findings), (0, 0));
    }

    #[test]
    fn different_repositories_have_no_trend() {
        let a = card("one", 0.9, &[]);
        let b = card("two", 0.5, &[]);
        assert_eq!(trend(&a, &b), None);
    }
}

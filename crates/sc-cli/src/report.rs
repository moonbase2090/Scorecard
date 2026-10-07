// SPDX-License-Identifier: MPL-2.0
//! Shared report rendering helpers.

use sc_core::Scorecard;

mod html;
pub(crate) use html::to_html;

/// How a verdict should read, shared by every renderer. Exit code and the
/// verdict field are untouched; this only picks the words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Fail,
    /// Every provided gate passed.
    Pass,
    /// Enforced gates passed; this many advisory gates (such as `sca`) failed.
    Advisory(usize),
    /// No gate was enforced and this many failed. Must never read as a clean
    /// pass.
    ReportOnly(usize),
}

impl Outcome {
    pub(crate) fn of(card: &Scorecard) -> Self {
        if sc_core::report_verdict(card) != "pass" {
            return Outcome::Fail;
        }
        let provided: Vec<_> = card
            .measures
            .gates
            .iter()
            .filter(|gate| provided(gate))
            .collect();
        let failing = provided.iter().filter(|gate| !gate.pass).count();
        if failing == 0 {
            Outcome::Pass
        } else if provided.iter().any(|gate| gate.enforced) {
            Outcome::Advisory(failing)
        } else {
            Outcome::ReportOnly(failing)
        }
    }

    /// "1 failing gate, none enforced" or "1 advisory gate failing".
    pub(crate) fn note(self) -> Option<String> {
        let noun = |n: usize| if n == 1 { "gate" } else { "gates" };
        match self {
            Outcome::ReportOnly(n) => Some(format!("{n} failing {}, none enforced", noun(n))),
            Outcome::Advisory(n) => Some(format!("{n} advisory {} failing", noun(n))),
            Outcome::Fail | Outcome::Pass => None,
        }
    }

    /// Verdict word for the page title and the flow strip.
    fn word(self, card: &Scorecard) -> &str {
        match self {
            Outcome::ReportOnly(_) => "report only",
            _ => sc_core::report_verdict(card),
        }
    }
}

/// Reason marker for gates the active pack does not evaluate. Such gates
/// carry no signal: they render as skipped and stay out of the pass ratio.
pub(crate) const NOT_PROVIDED: &str = "not provided by this pack";

fn provided(gate: &sc_core::Gate) -> bool {
    gate.reason.as_deref() != Some(NOT_PROVIDED)
}

/// Coverage counts as measured only when the coverage engine ran. Incomplete
/// coverage still leaves measured rows scored; only a missing coverage run
/// means no CRAP numbers are reported at all.
pub(crate) fn coverage_measured(card: &Scorecard) -> bool {
    card.engines
        .engines_run
        .iter()
        .any(|engine| engine == "coverage")
}

/// One sentence for every renderer when coverage was not measured.
pub(crate) const COVERAGE_NOT_MEASURED: &str =
    "Coverage was not measured, so no CRAP scores are reported and nothing is treated as 0% coverage.";

/// One line for a `--diff` report: how many paths were scored, and how many
/// other source paths remain in the tree. `None` when not a diff run or when
/// the tree count is unknown.
pub(crate) fn rest_of_tree(card: &Scorecard) -> Option<String> {
    if card.context.scope.mode != "diff" {
        return None;
    }
    let other = card.context.scope.other_paths?;
    let scored = card.context.scope.paths.len() as u64;
    Some(format!(
        "{} in this diff; {} other {} in the tree",
        plural(scored as usize, "path", "paths"),
        other,
        if other == 1 { "path" } else { "paths" }
    ))
}

/// Short terminal form of `rest_of_tree`; fits 80 columns.
pub(crate) fn rest_of_tree_short(card: &Scorecard) -> Option<String> {
    if card.context.scope.mode != "diff" {
        return None;
    }
    let other = card.context.scope.other_paths?;
    let scored = card.context.scope.paths.len() as u64;
    Some(format!(
        "{scored} path{} in this diff; {other} other in the tree",
        if scored == 1 { "" } else { "s" }
    ))
}

/// A diff run only scores changed functions, so it has no tree-wide CRAP
/// count. Say what the number covers and where the tree total lives
/// rather than inventing one. `None` outside diff scope.
pub(crate) fn diff_baseline(card: &Scorecard) -> Option<String> {
    if card.context.scope.mode != "diff" {
        return None;
    }
    let tree = match base_branch(card) {
        Some(branch) => format!("the latest push run of {branch}"),
        None => "a tree-scope run (sc analyze without --diff)".into(),
    };
    Some(format!(
        "diff scope: {} over threshold in this diff. The tree-wide count is on {tree}.",
        plural(
            card.measures.metrics.crap_over_threshold as usize,
            "function",
            "functions"
        )
    ))
}

/// Short terminal form of `diff_baseline`; fits 80 columns.
pub(crate) fn diff_baseline_short(card: &Scorecard) -> Option<String> {
    if card.context.scope.mode != "diff" {
        return None;
    }
    let tree = match base_branch(card) {
        Some(branch) => format!("latest {branch} push run"),
        None => "a run without --diff".into(),
    };
    Some(format!(
        "diff scope: {} over threshold here; tree count: {tree}",
        card.measures.metrics.crap_over_threshold
    ))
}

/// The branch a diff was taken against (`origin/main` reads as `main`).
/// `None` for a commit-ish base such as HEAD~1 or a SHA, which has no push
/// run of its own to point at.
fn base_branch(card: &Scorecard) -> Option<&str> {
    let base = card.context.scope.base.as_deref()?;
    let branch = base.strip_prefix("origin/").unwrap_or(base);
    let commitish = branch.is_empty()
        || branch.starts_with("HEAD")
        || branch.contains(['~', '^'])
        || (branch.len() >= 7 && branch.chars().all(|c| c.is_ascii_hexdigit()));
    (!commitish).then_some(branch)
}

/// Functions shown per finding from `evidence.functions`.
const FUNCTION_LIMIT: usize = 20;

/// The functions a finding lists in `evidence.functions` (for example the
/// functions `coverage.unmatched` found no coverage record for), as
/// ("file:line", symbol) pairs, plus how many more exist beyond what is
/// shown. The engine caps the list, so `evidence.unmatched` can give the
/// real total. Empty when the finding already names its one function.
pub(crate) fn evidence_functions(finding: &sc_core::Finding) -> (Vec<(String, String)>, usize) {
    let Some(items) = finding
        .evidence
        .get("functions")
        .and_then(|functions| functions.as_array())
    else {
        return (Vec::new(), 0);
    };
    let listed: Vec<(String, String)> = items
        .iter()
        .filter_map(|item| {
            let file = item.get("file")?.as_str()?;
            let symbol = item.get("symbol")?.as_str()?;
            let line = item
                .get("line")
                .or_else(|| item.get("span").and_then(|span| span.get("start_line")))
                .and_then(|line| line.as_u64());
            let location = match line {
                Some(line) => format!("{file}:{line}"),
                None => file.to_string(),
            };
            Some((location, symbol.to_string()))
        })
        .collect();
    if listed.len() == 1 && finding.symbol.as_deref() == Some(listed[0].1.as_str()) {
        return (Vec::new(), 0);
    }
    let total = finding
        .evidence
        .get("unmatched")
        .and_then(|count| count.as_u64())
        .map(|count| count as usize)
        .unwrap_or(listed.len())
        .max(listed.len());
    let shown: Vec<_> = listed.into_iter().take(FUNCTION_LIMIT).collect();
    let more = total - shown.len();
    (shown, more)
}

/// The raw command output a finding carries in `evidence.log` (lint and
/// test failures). The message is the first useful line; the log sits
/// behind a disclosure so the compile preamble never leads the card.
pub(crate) fn raw_log(finding: &sc_core::Finding) -> Option<&str> {
    finding
        .evidence
        .get("log")
        .and_then(|log| log.as_str())
        .map(str::trim_end)
        .filter(|log| !log.trim().is_empty())
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

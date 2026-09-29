// SPDX-License-Identifier: MPL-2.0
use sc_core::{crap_score, exceeds_threshold, CrapFunction, Finding};
use sc_graph::FunctionInfo;

use crate::coverage::CoverageData;

pub struct CrapOutcome {
    pub findings: Vec<Finding>,
    /// True when every analyzed function has a coverage record.
    pub coverage_complete: bool,
    pub worst: Vec<CrapFunction>,
    pub crap_max: f64,
    pub over: u64,
    /// Measured functions with CC at or above `new_fn_untested_cc` and coverage 0.
    pub untested: u64,
}

const CRAP_ACTION: &str = "Add tests covering branches or split the function";

pub fn evaluate(
    functions: &[FunctionInfo],
    coverage: Option<&CoverageData>,
    threshold: u32,
    untested_cc: u32,
    untested: impl Fn(&FunctionInfo) -> bool,
) -> CrapOutcome {
    let files: Vec<&str> = functions.iter().map(|item| item.file.as_str()).collect();
    let mut rows: Vec<CrapFunction> = functions
        .iter()
        .filter_map(|function| {
            let cov = coverage
                .and_then(|data| data.for_function_known(&function.file, &function.symbol, &files))?
                .clamp(0.0, 1.0);
            let crap = crap_score(function.cc, cov);
            Some(CrapFunction {
                symbol: function.symbol.clone(),
                file: function.file.clone(),
                cc: function.cc,
                coverage: cov,
                crap,
            })
        })
        .collect();

    rows.sort_by(|a, b| {
        b.crap
            .partial_cmp(&a.crap)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.symbol.cmp(&b.symbol))
    });

    let coverage_complete = functions.iter().all(|function| {
        coverage
            .and_then(|data| data.for_function_known(&function.file, &function.symbol, &files))
            .is_some()
    });

    let crap_max = rows.first().map(|row| row.crap).unwrap_or(0.0);
    let over = rows
        .iter()
        .filter(|row| exceeds_threshold(row.crap, threshold))
        .count() as u64;

    let mut findings = Vec::new();
    let mut untested_count = 0u64;
    for function in functions {
        let Some(row) = rows
            .iter()
            .find(|row| row.file == function.file && row.symbol == function.symbol)
        else {
            continue;
        };
        if exceeds_threshold(row.crap, threshold) {
            findings.push(Finding {
                id: format!("crap:{}:{}", function.file, function.symbol),
                rule: "crap.over_threshold".into(),
                engine: "crap".into(),
                severity: "error".into(),
                file: function.file.clone(),
                span: Some(function.span.clone()),
                symbol: Some(function.symbol.clone()),
                message: format!(
                    "CRAP {} (CC={}, cov={}%) exceeds threshold {}",
                    fmt_num(row.crap),
                    row.cc,
                    pct(row.coverage),
                    threshold,
                ),
                evidence: serde_json::json!({
                    "cc": row.cc,
                    "coverage": row.coverage,
                    "crap": row.crap,
                }),
                suggested_action: Some(CRAP_ACTION.into()),
                disposition: String::new(),
            });
        }
        if untested(function) && row.cc >= untested_cc && row.coverage == 0.0 {
            untested_count += 1;
            findings.push(Finding {
                id: format!("complexity:{}:{}", function.file, function.symbol),
                rule: "complexity.untested".into(),
                engine: "complexity".into(),
                severity: "error".into(),
                file: function.file.clone(),
                span: Some(function.span.clone()),
                symbol: Some(function.symbol.clone()),
                message: format!(
                    "CC {} with 0% coverage meets new_fn_untested_cc {}",
                    row.cc, untested_cc,
                ),
                evidence: serde_json::json!({
                    "cc": row.cc,
                    "coverage": row.coverage,
                    "new_fn_untested_cc": untested_cc,
                }),
                suggested_action: Some("Add tests covering this function or split it".into()),
                disposition: String::new(),
            });
        }
    }

    let worst: Vec<_> = rows.into_iter().take(10).collect();
    CrapOutcome {
        findings,
        coverage_complete,
        worst,
        crap_max,
        over,
        untested: untested_count,
    }
}

pub fn unmatched_count(functions: &[FunctionInfo], coverage: &CoverageData) -> u64 {
    unmatched_functions(functions, coverage).len() as u64
}

pub fn unmatched_functions<'a>(
    functions: &'a [FunctionInfo],
    coverage: &'a CoverageData,
) -> Vec<&'a FunctionInfo> {
    let files: Vec<&str> = functions.iter().map(|item| item.file.as_str()).collect();
    functions
        .iter()
        .filter(|function| {
            coverage
                .for_function_known(&function.file, &function.symbol, &files)
                .is_none()
        })
        .collect()
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
    use sc_core::Span;

    #[test]
    fn unmeasured_coverage_does_not_create_crap_rows_or_findings() {
        let functions = vec![FunctionInfo {
            file: "src/parse.rs".into(),
            symbol: "parse_input".into(),
            span: Span {
                start_line: 42,
                start_col: 1,
                end_line: 88,
                end_col: 2,
            },
            cc: 12,
        }];
        let outcome = evaluate(&functions, None, 30, 15, |_| true);
        assert!(!outcome.coverage_complete);
        assert_eq!(outcome.over, 0);
        assert_eq!(outcome.untested, 0);
        assert_eq!(outcome.crap_max, 0.0);
        assert!(outcome.worst.is_empty());
        assert!(outcome.findings.is_empty());
    }

    #[test]
    fn missing_function_record_is_not_treated_as_zero_coverage() {
        let functions = vec![FunctionInfo {
            file: "src/lib.rs".into(),
            symbol: "wide".into(),
            span: Span {
                start_line: 1,
                start_col: 1,
                end_line: 20,
                end_col: 2,
            },
            cc: 15,
        }];
        let coverage = CoverageData {
            functions: Vec::new(),
            line_rate: 0.5,
        };
        let outcome = evaluate(&functions, Some(&coverage), 30, 15, |_| true);
        assert!(!outcome.coverage_complete);
        assert_eq!(outcome.over, 0);
        assert_eq!(outcome.untested, 0);
        assert_eq!(outcome.crap_max, 0.0);
        assert!(outcome.worst.is_empty());
        assert!(outcome.findings.is_empty());
    }

    #[test]
    fn full_coverage_of_cc_11_stays_under_threshold() {
        let functions = vec![FunctionInfo {
            file: "src/lib.rs".into(),
            symbol: "classify".into(),
            span: Span {
                start_line: 1,
                start_col: 1,
                end_line: 4,
                end_col: 2,
            },
            cc: 11,
        }];
        let coverage = CoverageData {
            functions: vec![crate::coverage::CovFunction {
                file: "/tmp/crate/src/lib.rs".into(),
                demangled: "crate::classify".into(),
                coverage: 1.0,
            }],
            line_rate: 1.0,
        };
        let outcome = evaluate(&functions, Some(&coverage), 30, 15, |_| true);
        assert!(outcome.coverage_complete);
        assert_eq!(outcome.over, 0);
        assert_eq!(outcome.untested, 0);
        assert!(outcome.findings.is_empty());
        assert!((outcome.crap_max - 11.0).abs() < 1e-9);
    }

    #[test]
    fn measured_zero_coverage_over_threshold_stays_an_error() {
        let functions = vec![FunctionInfo {
            file: "src/lib.rs".into(),
            symbol: "wide".into(),
            span: Span {
                start_line: 1,
                start_col: 1,
                end_line: 20,
                end_col: 2,
            },
            cc: 12,
        }];
        let coverage = CoverageData {
            functions: vec![crate::coverage::CovFunction {
                file: "src/lib.rs".into(),
                demangled: "crate::wide".into(),
                coverage: 0.0,
            }],
            line_rate: 0.0,
        };
        let outcome = evaluate(&functions, Some(&coverage), 30, 15, |_| true);
        assert!(outcome.coverage_complete);
        assert_eq!(outcome.over, 1);
        assert_eq!(outcome.findings[0].rule, "crap.over_threshold");
        assert_eq!(outcome.findings[0].severity, "error");
        assert!(!outcome.findings[0].message.contains("not measured"));
    }

    #[test]
    fn partial_coverage_scores_measured_rows() {
        let functions = vec![
            FunctionInfo {
                file: "src/lib.rs".into(),
                symbol: "covered".into(),
                span: Span {
                    start_line: 1,
                    start_col: 1,
                    end_line: 20,
                    end_col: 2,
                },
                cc: 12,
            },
            FunctionInfo {
                file: "src/other.rs".into(),
                symbol: "missing".into(),
                span: Span {
                    start_line: 1,
                    start_col: 1,
                    end_line: 20,
                    end_col: 2,
                },
                cc: 12,
            },
        ];
        let coverage = CoverageData {
            functions: vec![crate::coverage::CovFunction {
                file: "src/lib.rs".into(),
                demangled: "crate::covered".into(),
                coverage: 0.0,
            }],
            line_rate: 0.5,
        };
        let outcome = evaluate(&functions, Some(&coverage), 30, 15, |_| true);
        assert!(!outcome.coverage_complete);
        assert_eq!(outcome.over, 1);
        assert_eq!(outcome.untested, 0);
        assert_eq!(outcome.crap_max, 156.0);
        assert_eq!(outcome.worst.len(), 1);
        assert_eq!(outcome.worst[0].symbol, "covered");
        assert_eq!(outcome.findings.len(), 1);
        assert_eq!(outcome.findings[0].rule, "crap.over_threshold");
    }

    #[test]
    fn cc_at_untested_floor_fails_even_when_crap_threshold_is_high() {
        let functions = vec![FunctionInfo {
            file: "src/lib.rs".into(),
            symbol: "wide".into(),
            span: Span {
                start_line: 1,
                start_col: 1,
                end_line: 20,
                end_col: 2,
            },
            cc: 15,
        }];
        let coverage = CoverageData {
            functions: vec![crate::coverage::CovFunction {
                file: "src/lib.rs".into(),
                demangled: "crate::wide".into(),
                coverage: 0.0,
            }],
            line_rate: 0.0,
        };
        let outcome = evaluate(&functions, Some(&coverage), 10_000, 15, |_| true);
        assert_eq!(outcome.over, 0);
        assert_eq!(outcome.untested, 1);
        assert_eq!(outcome.findings.len(), 1);
        assert_eq!(outcome.findings[0].rule, "complexity.untested");
        assert_eq!(outcome.findings[0].id, "complexity:src/lib.rs:wide");
        assert_eq!(outcome.findings[0].severity, "error");
        assert!(
            !outcome.findings[0]
                .message
                .contains("coverage not measured"),
            "{}",
            outcome.findings[0].message
        );
    }

    #[test]
    fn measured_zero_coverage_stays_error() {
        let functions = vec![FunctionInfo {
            file: "src/lib.rs".into(),
            symbol: "wide".into(),
            span: Span {
                start_line: 1,
                start_col: 1,
                end_line: 20,
                end_col: 2,
            },
            cc: 15,
        }];
        let coverage = CoverageData {
            functions: vec![crate::coverage::CovFunction {
                file: "src/lib.rs".into(),
                demangled: "crate::wide".into(),
                coverage: 0.0,
            }],
            line_rate: 0.0,
        };
        let outcome = evaluate(&functions, Some(&coverage), 10_000, 15, |_| true);
        assert_eq!(outcome.untested, 1);
        assert_eq!(outcome.findings[0].severity, "error");
        assert!(
            !outcome.findings[0].message.contains("not measured"),
            "{}",
            outcome.findings[0].message
        );
    }
}

// SPDX-License-Identifier: MPL-2.0
//! Function line coverage from `cargo llvm-cov --json`.
//!
//! Names are demangled and crate disambiguator hashes are stripped, then matched
//! to syn symbols. A miss is coverage 0 for that function.

use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct CovFunction {
    pub file: String,
    pub demangled: String,
    pub coverage: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoverageData {
    pub functions: Vec<CovFunction>,
    pub line_rate: f64,
}

impl CoverageData {
    pub fn for_function(&self, file: &str, symbol: &str) -> Option<f64> {
        let mut best: Option<f64> = None;
        for function in &self.functions {
            if file_matches(&function.file, file) && symbol_matches(&function.demangled, symbol) {
                best = Some(
                    best.map(|current| current.max(function.coverage))
                        .unwrap_or(function.coverage),
                );
            }
        }
        best
    }
}

pub fn parse_coverage_json(text: &str) -> Result<CoverageData, String> {
    let value: Value = serde_json::from_str(text).map_err(|err| err.to_string())?;
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .ok_or_else(|| "coverage json has no data".to_string())?;
    let functions = data
        .get("functions")
        .and_then(Value::as_array)
        .ok_or_else(|| "coverage json has no functions".to_string())?;

    let mut parsed = Vec::new();
    for function in functions {
        let name = function.get("name").and_then(Value::as_str).unwrap_or("");
        let demangled = normalize_demangled(name);
        let file = function
            .get("filenames")
            .and_then(Value::as_array)
            .and_then(|files| files.first())
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let regions = function.get("regions").and_then(Value::as_array);
        let count = function.get("count").and_then(as_i64).unwrap_or(0);
        let coverage = match regions {
            Some(regions) => {
                region_line_coverage(regions).unwrap_or(if count > 0 { 1.0 } else { 0.0 })
            }
            None => {
                if count > 0 {
                    1.0
                } else {
                    0.0
                }
            }
        };
        parsed.push(CovFunction {
            file,
            demangled,
            coverage,
        });
    }

    let line_rate = data
        .get("totals")
        .and_then(|totals| totals.get("lines"))
        .map(line_rate_from_summary)
        .unwrap_or(0.0);

    Ok(CoverageData {
        functions: parsed,
        line_rate,
    })
}

fn line_rate_from_summary(lines: &Value) -> f64 {
    if let Some(percent) = lines.get("percent").and_then(Value::as_f64) {
        return (percent / 100.0).clamp(0.0, 1.0);
    }
    let count = lines.get("count").and_then(as_i64).unwrap_or(0);
    let covered = lines.get("covered").and_then(as_i64).unwrap_or(0);
    if count <= 0 {
        0.0
    } else {
        (covered as f64 / count as f64).clamp(0.0, 1.0)
    }
}

fn region_line_coverage(regions: &[Value]) -> Option<f64> {
    let mut lines: BTreeMap<i64, i64> = BTreeMap::new();
    for region in regions {
        let Some(nums) = region.as_array() else {
            continue;
        };
        if nums.len() < 5 {
            continue;
        }
        let kind = nums.get(7).and_then(as_i64).unwrap_or(0);
        if kind != 0 {
            continue;
        }
        let start = as_i64(&nums[0])?;
        let end = as_i64(&nums[2])?;
        let count = as_i64(&nums[4]).unwrap_or(0);
        if end < start {
            continue;
        }
        for line in start..=end {
            let entry = lines.entry(line).or_insert(0);
            if count > *entry {
                *entry = count;
            }
        }
    }
    if lines.is_empty() {
        return None;
    }
    let covered = lines.values().filter(|count| **count > 0).count();
    Some(covered as f64 / lines.len() as f64)
}

fn as_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|n| i64::try_from(n).ok()))
        .or_else(|| value.as_f64().map(|n| n as i64))
}

pub fn normalize_demangled(raw: &str) -> String {
    let demangled = rustc_demangle::demangle(raw).to_string();
    let stripped = strip_disambiguators(&demangled);
    // `<CcVisitor as Visit>::visit_expr` is the syn symbol `CcVisitor::visit_expr`.
    if let Some(rest) = stripped.strip_prefix('<') {
        if let Some((ty, after)) = rest.split_once(" as ") {
            if let Some((_, method)) = after.split_once(">::") {
                let ty = ty.split('<').next().unwrap_or(ty).trim_end_matches("::");
                return format!("{ty}::{method}");
            }
        }
    }
    // `evaluate::<python::run>` is still `evaluate`. Deleting the brackets
    // used to glue the type arguments on as extra path segments.
    let cut = stripped.split('<').next().unwrap_or(&stripped);
    cut.trim_end_matches("::").to_string()
}

fn strip_disambiguators(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut rest = name;
    while let Some(start) = rest.find('[') {
        out.push_str(&rest[..start]);
        match rest[start + 1..].find(']') {
            Some(end) => rest = &rest[start + 1 + end + 1..],
            None => {
                out.push_str(&rest[start..]);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

fn path_after_crate(demangled: &str) -> &str {
    demangled
        .split_once("::")
        .map(|(_, rest)| rest)
        .unwrap_or(demangled)
}

pub fn symbol_matches(demangled: &str, symbol: &str) -> bool {
    path_after_crate(demangled) == symbol
}

pub fn file_matches(cov_file: &str, rel: &str) -> bool {
    let cov_file = cov_file.replace('\\', "/");
    let rel = rel.replace('\\', "/");
    cov_file == rel || cov_file.ends_with(&format!("/{rel}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demangle_and_match_classify() {
        let mangled = "_RNvCs5M70vpLA1I5_8covprobe8classify";
        let name = normalize_demangled(mangled);
        assert_eq!(name, "covprobe::classify");
        assert!(symbol_matches(&name, "classify"));
        assert!(!symbol_matches(&name, "other"));
    }

    #[test]
    fn region_coverage_and_file_match() {
        let json = r#"
        {
          "data": [{
            "functions": [{
              "name": "_RNvCs5M70vpLA1I5_8covprobe8classify",
              "count": 9,
              "regions": [[1, 1, 2, 13, 9, 0, 0, 0], [3, 16, 3, 21, 0, 0, 0, 0]],
              "filenames": ["/tmp/covprobe/src/lib.rs"]
            }],
            "totals": {"lines": {"count": 10, "covered": 5, "percent": 50.0}}
          }]
        }
        "#;
        let data = parse_coverage_json(json).unwrap();
        let cov = data.for_function("src/lib.rs", "classify").unwrap();
        assert!((cov - (2.0 / 3.0)).abs() < 1e-9);
        assert!((data.line_rate - 0.5).abs() < 1e-9);
    }

    #[test]
    fn method_path_matches_exactly() {
        assert!(symbol_matches("crate::Foo::bar", "Foo::bar"));
        assert!(!symbol_matches("crate::Foo::bar", "bar"));
    }

    #[test]
    fn trait_method_demangles_to_the_syn_symbol() {
        let raw = "_RNvXNtCsgmNkC5cO7tE_8sc_graph10complexityNtB2_9CcVisitorNtNtNtCsjHvgqTPRpCr_3syn3gen5visit5Visit10visit_expr";
        let full = rustc_demangle::demangle(raw).to_string();
        let name = normalize_demangled(raw);
        assert!(
            symbol_matches(&name, "complexity::CcVisitor::visit_expr"),
            "full={full} norm={name}"
        );
    }

    #[test]
    fn generic_instantiation_matches_the_base_symbol() {
        let raw = "_RINvNtCsh9m6iVWrboU_10sc_engines4crap8evaluateNCNvNtB4_6python3run0EB4_";
        let name = normalize_demangled(raw);
        assert!(symbol_matches(&name, "crap::evaluate"), "{name}");
        assert!(!symbol_matches(&name, "evaluate"), "{name}");
    }
}

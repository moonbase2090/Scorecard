// SPDX-License-Identifier: MPL-2.0
//! Function line coverage from `cargo llvm-cov --workspace --json`.
//!
//! Names are demangled and crate disambiguator hashes are stripped, then matched
//! to syn symbols. A miss means coverage was not measured for that function.

use sc_core::Finding;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

pub fn missing_finding(reason: &str, fix: &str) -> Finding {
    let reason = if reason.trim().is_empty() {
        "coverage data is unavailable"
    } else {
        reason
    };
    let fix = if fix.trim().is_empty() {
        "Enable coverage tooling for this pack, then re-run `sc analyze`"
    } else {
        fix
    };
    Finding {
        id: "coverage:missing".into(),
        rule: "coverage.missing".into(),
        engine: "coverage".into(),
        severity: "warning".into(),
        file: ".".into(),
        span: None,
        symbol: None,
        message: format!("coverage was not measured ({reason})"),
        evidence: serde_json::json!({}),
        suggested_action: Some(fix.into()),
        disposition: String::new(),
    }
}

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
    /// A coverage path is not shared with a shorter path when a longer known
    /// file is the real suffix. Among remaining matches, the tightest path
    /// wins. A higher number from another file does not replace it.
    pub fn for_function_known(&self, file: &str, symbol: &str, known: &[&str]) -> Option<f64> {
        let mut best: Option<(usize, f64)> = None;
        for function in &self.functions {
            if !symbol_matches(&function.demangled, symbol) {
                continue;
            }
            if !path_owned(&function.file, file, known.iter().copied()) {
                continue;
            }
            let Some(prefix) = path_prefix_len(&function.file, file) else {
                continue;
            };
            best = Some(match best {
                None => (prefix, function.coverage),
                Some((best_prefix, _)) if prefix < best_prefix => (prefix, function.coverage),
                Some((best_prefix, coverage)) if prefix == best_prefix => {
                    (prefix, coverage.max(function.coverage))
                }
                Some(kept) => kept,
            });
        }
        best.map(|(_, coverage)| coverage)
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
        if let Some((ty, method)) = rest.split_once(">::") {
            let ty = ty.split('<').next().unwrap_or(ty).trim_end_matches("::");
            return format!("{ty}::{method}");
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

/// Scored files plus paths that exist only so a longer report path can own its hits.
pub fn merge_known<'a>(
    files: impl IntoIterator<Item = &'a str>,
    extra: &'a [String],
) -> Vec<&'a str> {
    let mut seen = HashSet::new();
    let mut known = Vec::new();
    for path in files.into_iter().chain(extra.iter().map(String::as_str)) {
        if seen.insert(path) {
            known.push(path);
        }
    }
    known
}

/// True when `cov_file` is `rel`, or ends at a path boundary with `rel`,
/// and no longer known file is a better suffix of `cov_file`.
/// One report path maps to one project file. Built once per report.
pub fn file_owners<'a>(
    reports: impl IntoIterator<Item = &'a str>,
    known: &[&str],
) -> HashMap<String, String> {
    let index = KnownPaths::new(known);
    let mut map = HashMap::new();
    let mut seen = HashSet::new();
    for report in reports {
        let key = normalize_path(report);
        if !seen.insert(key.clone()) {
            continue;
        }
        if let Some(file) = index.owner(&key) {
            map.insert(key, file.to_string());
        }
    }
    map
}

pub fn report_owns(
    owners: &std::collections::HashMap<String, String>,
    report: &str,
    file: &str,
) -> bool {
    owners
        .get(&normalize_path(report))
        .is_some_and(|owner| owner == &normalize_path(file))
}

/// Known files grouped by basename. A report path can only belong to a file
/// with the same final component, so a vendor tree is not scanned per line.
struct KnownPaths<'a> {
    by_base: HashMap<String, Vec<(&'a str, String)>>,
}

impl<'a> KnownPaths<'a> {
    fn new(known: &[&'a str]) -> Self {
        let mut by_base: HashMap<String, Vec<(&str, String)>> = HashMap::new();
        for file in known {
            let norm = normalize_path(file);
            let base = norm.rsplit('/').next().unwrap_or("").to_string();
            by_base.entry(base).or_default().push((*file, norm));
        }
        Self { by_base }
    }

    fn owner(&self, report_norm: &str) -> Option<&'a str> {
        if report_norm.is_empty() {
            return None;
        }
        let base = report_norm.rsplit('/').next().unwrap_or("");
        let candidates = self.by_base.get(base)?;
        let mut best: Option<(usize, &'a str)> = None;
        for (file, norm) in candidates {
            let Some(prefix) = normalized_prefix_len(report_norm, norm) else {
                continue;
            };
            best = Some(match best {
                None => (prefix, *file),
                Some((best_prefix, _)) if prefix < best_prefix => (prefix, *file),
                Some(kept) => kept,
            });
        }
        if let Some((_, file)) = best {
            return Some(file);
        }
        // JaCoCo names the package plus the source file, a suffix of the project path.
        let mut suffix_hits = Vec::new();
        let marker = format!("/{report_norm}");
        for (file, norm) in candidates {
            if norm == report_norm || norm.ends_with(&marker) {
                suffix_hits.push(*file);
            }
        }
        if suffix_hits.len() == 1 {
            return Some(suffix_hits[0]);
        }
        if report_norm.contains('/') || candidates.len() != 1 {
            return None;
        }
        Some(candidates[0].0)
    }
}

fn normalized_prefix_len(cov: &str, rel: &str) -> Option<usize> {
    if rel.is_empty() {
        return None;
    }
    if cov == rel {
        return Some(0);
    }
    if cov.len() > rel.len()
        && cov.as_bytes()[cov.len() - rel.len() - 1] == b'/'
        && cov.ends_with(rel)
    {
        Some(cov.len() - rel.len() - 1)
    } else {
        None
    }
}

pub fn path_owned<'a>(cov_file: &str, rel: &str, known: impl IntoIterator<Item = &'a str>) -> bool {
    if path_prefix_len(cov_file, rel).is_none() {
        return false;
    }
    let rel_len = normalize_path(rel).len();
    for other in known {
        let other = normalize_path(other);
        if other.len() > rel_len && path_prefix_len(cov_file, &other).is_some() {
            return false;
        }
    }
    true
}

fn path_prefix_len(cov_file: &str, rel: &str) -> Option<usize> {
    let cov_file = normalize_path(cov_file);
    let rel = normalize_path(rel);
    if rel.is_empty() {
        return None;
    }
    if cov_file == rel {
        return Some(0);
    }
    let suffix = format!("/{rel}");
    if cov_file.ends_with(&suffix) {
        Some(cov_file.len() - suffix.len())
    } else {
        None
    }
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn many_vendor_paths_resolve_once_per_report_file() {
        let mut known = Vec::new();
        known.push("src/App.php".to_string());
        for index in 0..30_000 {
            known.push(format!("vendor/p{index}/File.php"));
        }
        known.push("vendor/pkg/App.php".to_string());
        let refs: Vec<&str> = known.iter().map(String::as_str).collect();
        let mut reports = vec!["src/App.php"; 60_000];
        reports.push("vendor/pkg/App.php");
        reports.push("com/example/App.java");
        let started = std::time::Instant::now();
        let owners = file_owners(reports, &refs);
        assert!(
            started.elapsed().as_secs() < 5,
            "owner map took {:?}",
            started.elapsed()
        );
        assert_eq!(
            owners.get("src/App.php").map(String::as_str),
            Some("src/App.php")
        );
        assert_eq!(
            owners.get("vendor/pkg/App.php").map(String::as_str),
            Some("vendor/pkg/App.php")
        );
        let java = ["src/main/java/com/example/App.java"];
        let jacoco = file_owners(["com/example/App.java", "com/example/App.java"], &java);
        assert_eq!(
            jacoco.get("com/example/App.java").map(String::as_str),
            Some("src/main/java/com/example/App.java")
        );
    }

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
        let cov = data
            .for_function_known("src/lib.rs", "classify", &[])
            .unwrap();
        assert!((cov - (2.0 / 3.0)).abs() < 1e-9);
        assert!((data.line_rate - 0.5).abs() < 1e-9);
    }

    #[test]
    fn a_longer_path_does_not_cover_a_shorter_one() {
        let data = CoverageData {
            functions: vec![
                CovFunction {
                    file: "/repo/src/lib.rs".into(),
                    demangled: "app::classify".into(),
                    coverage: 0.0,
                },
                CovFunction {
                    file: "/repo/helper/src/lib.rs".into(),
                    demangled: "helper::classify".into(),
                    coverage: 1.0,
                },
            ],
            line_rate: 0.5,
        };
        let known = ["src/lib.rs", "helper/src/lib.rs"];
        assert_eq!(
            data.for_function_known("src/lib.rs", "classify", &known),
            Some(0.0)
        );
        assert_eq!(
            data.for_function_known("helper/src/lib.rs", "classify", &known),
            Some(1.0)
        );
        let helper_only = CoverageData {
            functions: vec![CovFunction {
                file: "/repo/helper/src/lib.rs".into(),
                demangled: "helper::classify".into(),
                coverage: 1.0,
            }],
            line_rate: 1.0,
        };
        assert_eq!(
            helper_only.for_function_known("src/lib.rs", "classify", &known),
            None
        );
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
    fn pack_method_matches_the_syn_symbol() {
        let raw = "_RNvMNtCsh9m6iVWrboU_10sc_engines4packNtB2_6PackId6as_str";
        let full = rustc_demangle::demangle(raw).to_string();
        let name = normalize_demangled(raw);
        assert!(
            symbol_matches(&name, "pack::PackId::as_str"),
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

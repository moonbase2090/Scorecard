//! Line coverage reports for the non-Rust packs.
//!
//! Each parser returns statement hits inside a function span. Missing reports
//! leave CRAP at coverage 0.

use std::collections::BTreeSet;
use std::path::Path;

use sc_graph::FunctionInfo;

use crate::coverage::{CovFunction, CoverageData};

pub fn load(pack: &str, root: &Path, functions: &[FunctionInfo]) -> Option<CoverageData> {
    let dir = root.join(".sc").join("coverage");
    match pack {
        "node" => {
            read(&dir.join("coverage-final.json")).and_then(|text| istanbul(&text, functions))
        }
        "java" => read(&root.join("target/site/jacoco/jacoco.xml"))
            .or_else(|| read(&root.join("build/reports/jacoco/test/jacocoTestReport.xml")))
            .and_then(|text| jacoco(&text, functions)),
        "csharp" => {
            read(&dir.join("csharp.cobertura.xml")).and_then(|text| cobertura(&text, functions))
        }
        "php" => read(&dir.join("clover.xml")).and_then(|text| clover(&text, functions)),
        "bash" => find_named(&dir, &["cobertura.xml", "cov.xml", "kcov.xml"], 0)
            .and_then(|text| cobertura(&text, functions)),
        "cpp" => read(&dir.join("cpp.info")).and_then(|text| lcov(&text, functions)),
        "go" => None,
        _ => None,
    }
}

fn read(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn find_named(dir: &Path, names: &[&str], depth: u32) -> Option<String> {
    if depth > 4 || !dir.is_dir() {
        return None;
    }
    let entries: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().collect();
    for entry in &entries {
        let path = entry.path();
        if path.is_file()
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| names.contains(&name))
        {
            if let Some(text) = read(&path) {
                return Some(text);
            }
        }
    }
    for entry in &entries {
        let path = entry.path();
        if path.is_dir() {
            if let Some(text) = find_named(&path, names, depth + 1) {
                return Some(text);
            }
        }
    }
    None
}

pub fn istanbul(text: &str, functions: &[FunctionInfo]) -> Option<CoverageData> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let files = value.as_object()?;
    Some(from_hits(functions, |function| {
        let file = files
            .iter()
            .find(|(name, _)| file_match(name, &function.file))?;
        let map = file.1.get("statementMap")?.as_object()?;
        let hits = file.1.get("s")?.as_object()?;
        let mut hit = 0u32;
        let mut total = 0u32;
        for (id, loc) in map {
            let line = loc.pointer("/start/line")?.as_u64()? as u32;
            if line < function.span.start_line || line > function.span.end_line {
                continue;
            }
            total += 1;
            if hits.get(id).and_then(|count| count.as_u64()).unwrap_or(0) > 0 {
                hit += 1;
            }
        }
        (total > 0).then_some((hit, total))
    }))
}

pub fn jacoco(text: &str, functions: &[FunctionInfo]) -> Option<CoverageData> {
    let text = xml_lines(text);
    let mut source = String::new();
    let mut rows: Vec<(String, String, u32, u32)> = Vec::new();
    let mut method = String::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(name) = attr(trimmed, "sourcefilename") {
            source = name;
        }
        if trimmed.starts_with("<method ") {
            method = attr(trimmed, "name").unwrap_or_default();
        }
        if trimmed.contains("type=\"LINE\"") {
            let missed = attr(trimmed, "missed")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let covered = attr(trimmed, "covered")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            if !method.is_empty() && method != "<init>" && method != "<clinit>" {
                rows.push((source.clone(), method.clone(), covered, missed));
            }
        }
    }
    if rows.is_empty() {
        return None;
    }
    Some(from_hits(functions, |function| {
        rows.iter()
            .find(|(file, name, _, _)| file_match(file, &function.file) && *name == function.symbol)
            .map(|(_, _, covered, missed)| (*covered, covered + missed))
    }))
}

pub fn cobertura(text: &str, functions: &[FunctionInfo]) -> Option<CoverageData> {
    let text = xml_lines(text);
    let mut file = String::new();
    let mut lines: Vec<(String, u32, u32)> = Vec::new();
    for raw in text.lines() {
        let trimmed = raw.trim();
        if trimmed.starts_with("<class ") {
            if let Some(name) = attr(trimmed, "filename") {
                file = name;
            }
        }
        if trimmed.starts_with("<line ") {
            let number = attr(trimmed, "number")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let hits = attr(trimmed, "hits")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            if number > 0 {
                lines.push((file.clone(), number, hits));
            }
        }
    }
    if lines.is_empty() {
        return None;
    }
    Some(from_hits(functions, |function| {
        let mut hit = 0u32;
        let mut total = 0u32;
        for (name, line, hits) in &lines {
            if !file_match(name, &function.file) {
                continue;
            }
            if *line < function.span.start_line || *line > function.span.end_line {
                continue;
            }
            total += 1;
            if *hits > 0 {
                hit += 1;
            }
        }
        (total > 0).then_some((hit, total))
    }))
}

pub fn clover(text: &str, functions: &[FunctionInfo]) -> Option<CoverageData> {
    cobertura(text, functions).or_else(|| {
        let text = xml_lines(text);
        let mut file = String::new();
        let mut lines: Vec<(String, u32, u32)> = Vec::new();
        for raw in text.lines() {
            let trimmed = raw.trim();
            if trimmed.starts_with("<file ") {
                file = attr(trimmed, "name").unwrap_or_default();
            }
            if trimmed.starts_with("<line ") {
                let number = attr(trimmed, "num")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                let count = attr(trimmed, "count")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                if number > 0 {
                    lines.push((file.clone(), number, count));
                }
            }
        }
        if lines.is_empty() {
            return None;
        }
        Some(from_hits(functions, |function| {
            let mut hit = 0u32;
            let mut total = 0u32;
            for (name, line, count) in &lines {
                if !file_match(name, &function.file)
                    || *line < function.span.start_line
                    || *line > function.span.end_line
                {
                    continue;
                }
                total += 1;
                if *count > 0 {
                    hit += 1;
                }
            }
            (total > 0).then_some((hit, total))
        }))
    })
}

pub fn lcov(text: &str, functions: &[FunctionInfo]) -> Option<CoverageData> {
    let mut file = String::new();
    let mut lines: Vec<(String, u32, u32)> = Vec::new();
    for raw in text.lines() {
        if let Some(name) = raw.strip_prefix("SF:") {
            file = name.to_string();
        } else if let Some(rest) = raw.strip_prefix("DA:") {
            let mut parts = rest.split(',');
            let line = parts.next().unwrap_or("0").parse::<u32>().unwrap_or(0);
            let hits = parts.next().unwrap_or("0").parse::<u32>().unwrap_or(0);
            if line > 0 {
                lines.push((file.clone(), line, hits));
            }
        }
    }
    if lines.is_empty() {
        return None;
    }
    Some(from_hits(functions, |function| {
        let mut hit = 0u32;
        let mut total = 0u32;
        for (name, line, hits) in &lines {
            if !file_match(name, &function.file)
                || *line < function.span.start_line
                || *line > function.span.end_line
            {
                continue;
            }
            total += 1;
            if *hits > 0 {
                hit += 1;
            }
        }
        (total > 0).then_some((hit, total))
    }))
}

fn from_hits(
    functions: &[FunctionInfo],
    hits: impl Fn(&FunctionInfo) -> Option<(u32, u32)>,
) -> CoverageData {
    let mut covered = Vec::new();
    let mut seen = BTreeSet::new();
    for function in functions {
        let key = format!("{}:{}", function.file, function.symbol);
        if !seen.insert(key) {
            continue;
        }
        let coverage = match hits(function) {
            Some((hit, total)) if total > 0 => f64::from(hit) / f64::from(total),
            _ => 0.0,
        };
        covered.push(CovFunction {
            file: function.file.clone(),
            demangled: function.symbol.clone(),
            coverage,
        });
    }
    let line_rate = if covered.is_empty() {
        0.0
    } else {
        covered.iter().map(|row| row.coverage).sum::<f64>() / covered.len() as f64
    };
    CoverageData {
        functions: covered,
        line_rate,
    }
}

fn file_match(report: &str, function_file: &str) -> bool {
    let report = report.replace('\\', "/");
    let function_file = function_file.replace('\\', "/");
    if report == function_file || report.ends_with(&format!("/{function_file}")) {
        return true;
    }
    // JaCoCo records the source basename. Cobertura sometimes does too.
    let report_base = report.rsplit('/').next().unwrap_or(report.as_str());
    let function_base = function_file
        .rsplit('/')
        .next()
        .unwrap_or(function_file.as_str());
    !report_base.is_empty() && report_base == function_base
}

fn xml_lines(text: &str) -> String {
    text.replace("><", ">\n<")
}

fn attr(line: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let start = line.find(&needle)? + needle.len();
    let end = line[start..].find('"')? + start;
    Some(unescape(&line[start..end]))
}

fn unescape(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::Span;

    fn sample_java() -> FunctionInfo {
        FunctionInfo {
            file: "src/main/java/App.java".into(),
            symbol: "choose".into(),
            span: Span {
                start_line: 2,
                start_col: 1,
                end_line: 7,
                end_col: 1,
            },
            cc: 2,
        }
    }

    fn sample() -> FunctionInfo {
        FunctionInfo {
            file: "src/app.js".into(),
            symbol: "choose".into(),
            span: Span {
                start_line: 2,
                start_col: 1,
                end_line: 4,
                end_col: 1,
            },
            cc: 2,
        }
    }

    #[test]
    fn istanbul_counts_a_hit_and_a_miss() {
        let text = r#"{"src/app.js":{"statementMap":{"0":{"start":{"line":2}},"1":{"start":{"line":3}}},"s":{"0":1,"1":0}}}"#;
        let data = istanbul(text, &[sample()]).unwrap();
        let cov = data.for_function("src/app.js", "choose").unwrap();
        assert!((cov - 0.5).abs() < 1e-9, "{cov}");
    }

    #[test]
    fn jacoco_uses_the_method_line_counter() {
        let text = r#"
            <class name="App" sourcefilename="app.js">
              <method name="choose" desc="(I)Ljava/lang/String;">
                <counter type="LINE" missed="1" covered="1"/>
              </method>
            </class>
        "#;
        let data = jacoco(text, &[sample()]).unwrap();
        let cov = data.for_function("src/app.js", "choose").unwrap();
        assert!((cov - 0.5).abs() < 1e-9, "{cov}");
    }

    #[test]
    fn jacoco_reads_a_single_line_report() {
        let text = r#"<?xml version="1.0"?><report name="cov-java"><class name="App" sourcefilename="App.java"><method name="&lt;init&gt;" desc="()V" line="1"><counter type="LINE" missed="1" covered="0"/></method><method name="choose" desc="(I)Ljava/lang/String;" line="3"><counter type="LINE" missed="1" covered="2"/></method></class></report>"#;
        let data = jacoco(text, &[sample_java()]).unwrap();
        let cov = data
            .for_function("src/main/java/App.java", "choose")
            .unwrap();
        assert!((cov - (2.0 / 3.0)).abs() < 1e-9, "{cov}");
    }

    #[test]
    fn cobertura_counts_lines_inside_the_span() {
        let text = r#"
            <class filename="src/app.js">
              <line number="2" hits="1"/>
              <line number="3" hits="0"/>
            </class>
        "#;
        let data = cobertura(text, &[sample()]).unwrap();
        let cov = data.for_function("src/app.js", "choose").unwrap();
        assert!((cov - 0.5).abs() < 1e-9, "{cov}");
    }

    #[test]
    fn clover_uses_num_and_count() {
        let text = r#"
            <file name="/src/src/app.js">
              <line num="2" count="1"/>
              <line num="3" count="0"/>
            </file>
        "#;
        let data = clover(text, &[sample()]).unwrap();
        let cov = data.for_function("src/app.js", "choose").unwrap();
        assert!((cov - 0.5).abs() < 1e-9, "{cov}");
    }

    #[test]
    fn lcov_uses_da_records() {
        let text = "SF:/src/src/app.js\nDA:2,1\nDA:3,0\nend_of_record\n";
        let data = lcov(text, &[sample()]).unwrap();
        let cov = data.for_function("src/app.js", "choose").unwrap();
        assert!((cov - 0.5).abs() < 1e-9, "{cov}");
    }
}

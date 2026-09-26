// SPDX-License-Identifier: MPL-2.0
use std::path::Path;

use sc_core::{Finding, Span};
use serde_json::Value;

use crate::command::brief;

pub fn parse_compiler_messages(root: &Path, stdout: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if !line.starts_with('{') {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value.get("reason").and_then(Value::as_str) != Some("compiler-message") {
            continue;
        }
        let Some(message) = value.get("message") else {
            continue;
        };
        if message.get("level").and_then(Value::as_str) != Some("error") {
            continue;
        }
        let text = message
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("compiler error");
        let code = message
            .get("code")
            .and_then(|code| code.get("code"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let parsed = primary_span(message);
        let file = parsed
            .as_ref()
            .map(|span| normalize_file(root, &span.file))
            .filter(|file| !file.is_empty())
            .unwrap_or_else(|| ".".to_string());
        let span = parsed.map(|span| span.span);
        let id = match &span {
            Some(span) => format!("compile:{file}:{}:{}", span.start_line, span.start_col),
            None => format!("compile:{file}:error"),
        };
        let rendered = if code.is_empty() {
            text.to_string()
        } else {
            format!("error[{code}]: {text}")
        };
        let evidence = if code.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::json!({"code": code})
        };
        findings.push(Finding {
            id,
            rule: "compile.error".into(),
            engine: "compile".into(),
            severity: "error".into(),
            file,
            span,
            symbol: None,
            message: rendered,
            evidence,
            suggested_action: Some("Fix the compiler error".into()),
            disposition: String::new(),
        });
    }
    findings
}

pub fn generic_compile_failure(stdout: &str, stderr: &str) -> Finding {
    let detail = {
        let from_err = brief(stderr);
        if from_err.is_empty() {
            let from_out = brief(stdout);
            if from_out.is_empty() || from_out.starts_with('{') {
                "cargo check failed".to_string()
            } else {
                from_out
            }
        } else {
            from_err
        }
    };
    Finding {
        id: "compile:.:error".into(),
        rule: "compile.error".into(),
        engine: "compile".into(),
        severity: "error".into(),
        file: ".".into(),
        span: None,
        symbol: None,
        message: detail,
        evidence: serde_json::json!({}),
        suggested_action: Some("Fix the compiler error".into()),
        disposition: String::new(),
    }
}

struct ParsedSpan {
    file: String,
    span: Span,
}

fn primary_span(message: &Value) -> Option<ParsedSpan> {
    let spans = message.get("spans")?.as_array()?;
    let span = spans
        .iter()
        .find(|span| span.get("is_primary").and_then(Value::as_bool) == Some(true))
        .or_else(|| spans.first())?;
    let file = span.get("file_name")?.as_str()?.to_string();
    let start_line = span.get("line_start")?.as_u64()? as u32;
    let start_col = span.get("column_start")?.as_u64()? as u32;
    let end_line = span.get("line_end")?.as_u64()? as u32;
    let end_col = span.get("column_end")?.as_u64()? as u32;
    Some(ParsedSpan {
        file,
        span: Span {
            start_line,
            start_col,
            end_line,
            end_col,
        },
    })
}

pub fn normalize_file(root: &Path, file: &str) -> String {
    if file.is_empty() || file == "." {
        return file.to_string();
    }
    let path = Path::new(file);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    if let Ok(rel) = absolute.strip_prefix(root) {
        return rel.to_string_lossy().replace('\\', "/");
    }
    file.replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parses_a_primary_error_span() {
        let line = r#"{"reason":"compiler-message","message":{"level":"error","message":"cannot find value `missing` in this scope","code":{"code":"E0425"},"spans":[{"file_name":"src/lib.rs","line_start":4,"line_end":4,"column_start":5,"column_end":12,"is_primary":true}]}}"#;
        let findings = parse_compiler_messages(Path::new("/tmp/crate"), line);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, "compile.error");
        assert_eq!(findings[0].file, "src/lib.rs");
        assert_eq!(findings[0].span.as_ref().unwrap().start_line, 4);
        assert_eq!(findings[0].id, "compile:src/lib.rs:4:5");
        assert_eq!(findings[0].evidence["code"], "E0425");
    }
}

// SPDX-License-Identifier: MIT
//! MCP server for scorecard tools: `analyze_paths`, `analyze_diff`, `explain`,
//! `list_findings`, `rerun_gate`, `doctor`, and `compare_to_baseline`.
//! There is no chat tool.

use std::path::{Path, PathBuf};
use std::time::Duration;

use sc_core::{gates_failing_on_new_findings, Baseline, Finding, Scorecard};
use sc_engines::{analyze, AnalyzeRequest};
use serde_json::{json, Value};

pub fn handle(message: &Value, cwd: &Path) -> Option<Value> {
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    if method.starts_with("notifications/") {
        return None;
    }
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "sc-mcp", "version": env!("CARGO_PKG_VERSION")}
        }),
        "tools/list" => json!({"tools": tools()}),
        "tools/call" => call_tool(cwd, message.get("params").unwrap_or(&Value::Null)),
        _ => {
            return Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": format!("unknown method {method}")}
            }));
        }
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

fn tools() -> Vec<Value> {
    vec![
        tool(
            "analyze_paths",
            "Analyze explicit source paths and return a scorecard",
            json!({"type":"object","properties":{"paths":{"type":"array","items":{"type":"string"}},"spec_ref":{"type":"string"}},"required":["paths"]}),
        ),
        tool(
            "analyze_diff",
            "Analyze a git diff and return a scorecard",
            json!({"type":"object","properties":{"base":{"type":"string"},"head":{"type":"string"},"spec_ref":{"type":"string"}}}),
        ),
        tool(
            "explain",
            "Explain one finding from the last scorecard",
            json!({"type":"object","properties":{"finding_id":{"type":"string"}},"required":["finding_id"]}),
        ),
        tool(
            "list_findings",
            "List findings from the last scorecard",
            json!({"type":"object","properties":{"scorecard_id":{"type":"string"},"severity":{"type":"string"}}}),
        ),
        tool(
            "rerun_gate",
            "Re-run the analysis and return one gate's result with its findings",
            json!({"type":"object","properties":{"gate":{"type":"string"}},"required":["gate"]}),
        ),
        tool(
            "doctor",
            "List the tools the last analysis could not run, with the fix for each",
            json!({"type":"object","properties":{}}),
        ),
        tool(
            "compare_to_baseline",
            "Compare the last scorecard with .sc/baseline.json: new and resolved findings, and gates that fail on new findings",
            json!({"type":"object","properties":{}}),
        ),
    ]
}

fn tool(name: &str, description: &str, schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": schema,
    })
}

#[inline(never)]
fn call_tool(cwd: &Path, params: &Value) -> Value {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    let body = match name {
        "analyze_paths" => analyze_paths(cwd, &args),
        "analyze_diff" => analyze_diff(cwd, &args),
        "explain" => explain(cwd, &args),
        "list_findings" => list_findings(cwd, &args),
        "rerun_gate" => rerun_gate(cwd, &args),
        "doctor" => doctor(cwd),
        "compare_to_baseline" => compare_to_baseline(cwd),
        _ => Err(format!("unknown tool {name}")),
    };
    match body {
        Ok(text) => json!({"content": [{"type": "text", "text": text}], "isError": false}),
        Err(err) => json!({"content": [{"type": "text", "text": err}], "isError": true}),
    }
}

fn analyze_paths(cwd: &Path, args: &Value) -> Result<String, String> {
    let paths = args
        .get("paths")
        .and_then(Value::as_array)
        .ok_or("paths must be an array")?;
    let path_list: Vec<String> = paths
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    let spec = args
        .get("spec_ref")
        .and_then(Value::as_str)
        .map(PathBuf::from);
    let intent = args
        .get("intent")
        .and_then(Value::as_str)
        .map(str::to_string);
    run_analyze(cwd, None, None, path_list, spec, intent)
}

fn analyze_diff(cwd: &Path, args: &Value) -> Result<String, String> {
    let base = args
        .get("base")
        .and_then(Value::as_str)
        .unwrap_or("AUTO")
        .to_string();
    let head = args.get("head").and_then(Value::as_str).map(str::to_string);
    let spec = args
        .get("spec_ref")
        .and_then(Value::as_str)
        .map(PathBuf::from);
    let intent = args
        .get("intent")
        .and_then(Value::as_str)
        .map(str::to_string);
    run_analyze(cwd, Some(base), head, Vec::new(), spec, intent)
}

fn run_analyze(
    cwd: &Path,
    diff_base: Option<String>,
    diff_head: Option<String>,
    path_list: Vec<String>,
    spec_path: Option<PathBuf>,
    intent: Option<String>,
) -> Result<String, String> {
    let card = analyze_crate(cwd, diff_base, diff_head, path_list, spec_path, intent)?;
    serde_json::to_string(&card).map_err(|err| err.to_string())
}

fn analyze_crate(
    cwd: &Path,
    diff_base: Option<String>,
    diff_head: Option<String>,
    path_list: Vec<String>,
    spec_path: Option<PathBuf>,
    intent: Option<String>,
) -> Result<Scorecard, String> {
    let root = find_crate(cwd)?;
    let config = sc_core::load_config_file(sc_core::resolve_config_path(None, &root).as_deref())?;
    let output = analyze(AnalyzeRequest {
        root,
        repo: ".".into(),
        fail_on: config.gates.fail_on.clone(),
        budget: Duration::from_secs(120),
        diff_base,
        diff_head,
        path_list,
        spec_path,
        mutation_override: None,
        llm_override: Some(false),
        intent,
        progress: None,
        config,
    });
    Ok(output.scorecard)
}

fn rerun_gate(cwd: &Path, args: &Value) -> Result<String, String> {
    let gate = args
        .get("gate")
        .and_then(Value::as_str)
        .ok_or("gate is required")?;
    let card = analyze_crate(cwd, None, None, Vec::new(), None, None)?;
    let result = card
        .measures
        .gates
        .iter()
        .find(|result| result.id == gate)
        .ok_or_else(|| {
            let ids: Vec<&str> = card.measures.gates.iter().map(|g| g.id.as_str()).collect();
            format!("no gate {gate}; this run has: {}", ids.join(", "))
        })?;
    let findings: Vec<&Finding> = card
        .sections
        .findings
        .iter()
        .filter(|finding| finding.engine == engine_for_gate(gate))
        .collect();
    serde_json::to_string(&json!({
        "gate": result.id,
        "pass": result.pass,
        "enforced": result.enforced,
        "reason": result.reason,
        "findings": findings,
    }))
    .map_err(|err| err.to_string())
}

/// The engine whose findings explain a gate. `types` is reported by `compile`.
fn engine_for_gate(gate: &str) -> &str {
    if gate == "types" {
        "compile"
    } else {
        gate
    }
}

fn doctor(cwd: &Path) -> Result<String, String> {
    let card = last_card(cwd)?;
    let missing: Vec<Value> = card
        .sections
        .findings
        .iter()
        .filter(|finding| finding.rule == "engine.unavailable")
        .map(|finding| {
            json!({
                "engine": finding.engine,
                "message": finding.message,
                "fix": finding.suggested_action,
            })
        })
        .collect();
    serde_json::to_string(&missing).map_err(|err| err.to_string())
}

fn compare_to_baseline(cwd: &Path) -> Result<String, String> {
    let card = last_card(cwd)?;
    let text = find_file(cwd, ".sc/baseline.json")
        .ok_or("no baseline; run analyze --write-baseline first")?;
    let baseline: Baseline =
        serde_json::from_str(&std::fs::read_to_string(text).map_err(|err| err.to_string())?)
            .map_err(|err| err.to_string())?;
    let new: Vec<&Finding> = card
        .sections
        .findings
        .iter()
        .filter(|finding| !baseline.contains(finding))
        .collect();
    let current: std::collections::BTreeSet<String> = card
        .sections
        .findings
        .iter()
        .map(Finding::fingerprint)
        .collect();
    let resolved = baseline
        .fingerprints
        .iter()
        .filter(|fingerprint| !current.contains(*fingerprint))
        .count();
    serde_json::to_string(&json!({
        "new_findings": new,
        "resolved_findings": resolved,
        "gates_failing_on_new_findings": gates_failing_on_new_findings(&card, &baseline),
    }))
    .map_err(|err| err.to_string())
}

fn last_card(cwd: &Path) -> Result<Scorecard, String> {
    serde_json::from_value(last_scorecard(cwd)?).map_err(|err| err.to_string())
}

/// The nearest `relative` file at or above `cwd`, like `last-scorecard.json`.
fn find_file(cwd: &Path, relative: &str) -> Option<PathBuf> {
    let mut dir = cwd.to_path_buf();
    loop {
        let path = dir.join(relative);
        if path.is_file() {
            return Some(path);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn explain(cwd: &Path, args: &Value) -> Result<String, String> {
    let id = args
        .get("finding_id")
        .and_then(Value::as_str)
        .ok_or("finding_id is required")?;
    let card = last_scorecard(cwd)?;
    let finding = card
        .get("findings")
        .and_then(Value::as_array)
        .and_then(|findings| findings.iter().find(|finding| finding["id"] == id))
        .ok_or_else(|| format!("finding {id} is not in the last scorecard"))?;
    let why = finding["message"].as_str().unwrap_or("");
    let next = finding["suggested_action"].as_str().unwrap_or("");
    Ok(serde_json::to_string(&json!({
        "finding": finding,
        "why": why,
        "suggested_next": next,
    }))
    .unwrap_or_else(|_| "{}".into()))
}

#[inline(never)]
fn list_findings(cwd: &Path, args: &Value) -> Result<String, String> {
    let card = last_scorecard(cwd)?;
    if let Some(id) = args.get("scorecard_id").and_then(Value::as_str) {
        if card["id"].as_str() != Some(id) {
            return Err(format!("last scorecard id is not {id}"));
        }
    }
    let severity = args.get("severity").and_then(Value::as_str);
    let findings: Vec<&Value> = card
        .get("findings")
        .and_then(Value::as_array)
        .map(|findings| {
            findings
                .iter()
                .filter(|finding| match severity {
                    Some(severity) => finding["severity"].as_str() == Some(severity),
                    None => true,
                })
                .collect()
        })
        .unwrap_or_default();
    serde_json::to_string(&findings).map_err(|err| err.to_string())
}

fn last_scorecard(cwd: &Path) -> Result<Value, String> {
    let mut dir = cwd.to_path_buf();
    loop {
        let path = dir.join(".sc").join("last-scorecard.json");
        if path.is_file() {
            let text = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
            return serde_json::from_str(&text).map_err(|err| err.to_string());
        }
        if !dir.pop() {
            break;
        }
    }
    Err("no last scorecard; run analyze first".into())
}

fn find_crate(cwd: &Path) -> Result<PathBuf, String> {
    let mut dir = cwd.to_path_buf();
    loop {
        if dir.join("Cargo.toml").is_file() {
            return Ok(dir);
        }
        if !dir.pop() {
            break;
        }
    }
    Err("no Cargo.toml found from the working directory".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(engine: &str, rule: &str, message: &str) -> Finding {
        Finding {
            id: message.into(),
            rule: rule.into(),
            engine: engine.into(),
            severity: "error".into(),
            file: "src/lib.rs".into(),
            span: None,
            symbol: None,
            message: message.into(),
            evidence: json!({}),
            suggested_action: Some(format!("fix {message}")),
            disposition: String::new(),
        }
    }

    fn write_report(dir: &Path, card: &Scorecard, baseline: Option<&Baseline>) {
        std::fs::create_dir_all(dir.join(".sc")).unwrap();
        std::fs::write(
            dir.join(".sc/last-scorecard.json"),
            serde_json::to_string(card).unwrap(),
        )
        .unwrap();
        if let Some(baseline) = baseline {
            std::fs::write(
                dir.join(".sc/baseline.json"),
                serde_json::to_string(baseline).unwrap(),
            )
            .unwrap();
        }
    }

    #[test]
    fn compare_to_baseline_reports_new_findings_and_the_gate_they_fail() {
        let dir = std::env::temp_dir().join(format!("sc-mcp-compare-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let known = finding("tests", "test.failed", "known failure");
        let fresh = finding("tests", "test.failed", "new failure");
        let mut card = Scorecard::skeleton("repo", 30);
        card.measures.gates = vec![sc_core::Gate {
            id: "tests".into(),
            pass: false,
            enforced: true,
            reason: None,
        }];
        card.sections.findings = vec![known.clone(), fresh];
        write_report(&dir, &card, Some(&Baseline::from_findings(&[known])));
        let out: Value = serde_json::from_str(&compare_to_baseline(&dir).unwrap()).unwrap();
        assert_eq!(out["new_findings"].as_array().unwrap().len(), 1);
        assert_eq!(out["new_findings"][0]["message"], "new failure");
        assert_eq!(out["resolved_findings"], 0);
        assert_eq!(out["gates_failing_on_new_findings"], json!(["tests"]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn doctor_lists_each_engine_that_could_not_run_with_its_fix() {
        let dir = std::env::temp_dir().join(format!("sc-mcp-doctor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut card = Scorecard::skeleton("repo", 30);
        card.sections.findings = vec![
            finding("lint", "engine.unavailable", "shellcheck is not installed"),
            finding("tests", "test.failed", "a real failure"),
        ];
        write_report(&dir, &card, None);
        let out: Value = serde_json::from_str(&doctor(&dir).unwrap()).unwrap();
        assert_eq!(out.as_array().unwrap().len(), 1);
        assert_eq!(out[0]["engine"], "lint");
        assert_eq!(out[0]["fix"], "fix shellcheck is not installed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rerun_gate_returns_one_gate_or_names_the_gates_that_exist() {
        let dir = std::env::temp_dir().join(format!("sc-mcp-rerun-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"rerun_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "pub fn value() -> i32 { 1 }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn value_is_one() { assert_eq!(super::value(), 1); }\n}\n",
        )
        .unwrap();
        let out: Value =
            serde_json::from_str(&rerun_gate(&dir, &json!({"gate": "tests"})).unwrap()).unwrap();
        assert_eq!(out["gate"], "tests");
        assert_eq!(out["pass"], true, "{out}");
        let err = rerun_gate(&dir, &json!({"gate": "nope"})).unwrap_err();
        assert!(
            err.contains("no gate nope") && err.contains("tests"),
            "{err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lists_the_scorecard_tools_and_no_chat() {
        let response = handle(
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
            Path::new("."),
        )
        .unwrap();
        let names: Vec<_> = response["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            vec![
                "analyze_paths",
                "analyze_diff",
                "explain",
                "list_findings",
                "rerun_gate",
                "doctor",
                "compare_to_baseline"
            ]
        );
    }

    #[test]
    fn unknown_tool_is_an_error_and_list_findings_filters() {
        let bad = call_tool(Path::new("."), &json!({"name": "chat", "arguments": {}}));
        assert_eq!(bad["isError"], true);
        let dir = std::env::temp_dir().join(format!("sc-mcp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".sc")).unwrap();
        std::fs::write(
            dir.join(".sc/last-scorecard.json"),
            r#"{"id":"abc","findings":[{"id":"f1","fingerprint":"stable-fingerprint","severity":"error","message":"boom","suggested_action":"fix"}]}"#,
        )
        .unwrap();
        let listed = list_findings(&dir, &json!({"severity": "error"})).unwrap();
        assert!(listed.contains("f1"));
        let listed: Value = serde_json::from_str(&listed).unwrap();
        assert_eq!(listed[0]["fingerprint"], "stable-fingerprint");
        let explained = explain(&dir, &json!({"finding_id": "f1"})).unwrap();
        assert!(explained.contains("boom"));
        assert!(list_findings(&dir, &json!({"scorecard_id": "nope"})).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

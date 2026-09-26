//! MCP server for four scorecard tools.
//!
//! `analyze_paths`, `analyze_diff`, `explain`, and `list_findings`.
//! There is no chat tool.

use std::path::{Path, PathBuf};
use std::time::Duration;

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
            "serverInfo": {"name": "sc", "version": env!("CARGO_PKG_VERSION")}
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
    ]
}

fn tool(name: &str, description: &str, schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": schema,
    })
}

fn call_tool(cwd: &Path, params: &Value) -> Value {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    let body = match name {
        "analyze_paths" => analyze_paths(cwd, &args),
        "analyze_diff" => analyze_diff(cwd, &args),
        "explain" => explain(cwd, &args),
        "list_findings" => list_findings(cwd, &args),
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
    let root = find_crate(cwd)?;
    let config = sc_core::load_config_file(sc_core::resolve_config_path(None, &root).as_deref())
        .map_err(|err| err)?;
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
        config,
    });
    serde_json::to_string(&output.scorecard).map_err(|err| err.to_string())
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

    #[test]
    fn lists_four_tools_and_no_chat() {
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
            vec!["analyze_paths", "analyze_diff", "explain", "list_findings"]
        );
    }
}

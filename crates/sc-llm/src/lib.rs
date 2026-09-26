//! Optional spec-gap review over an OpenAI-compatible chat endpoint.
//!
//! The built-in default endpoint is local Ollama. When `XAI_API_KEY` is set
//! and the endpoint is still that default, the call uses SpaceXAI
//! (`https://api.x.ai/v1`, model `grok-4.5`). The model may report spec gaps.
//! It does not score CRAP or mutation.

use std::path::Path;
use std::time::Duration;

use serde_json::{json, Value};

pub const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:11434/v1";
pub const SPACEXAI_ENDPOINT: &str = "https://api.x.ai/v1";
pub const SPACEXAI_MODEL: &str = "grok-4.5";
const MAX_TOOL_ROUNDS: usize = 12;
const MAX_TOKENS: u32 = 2000;
const MAX_SPEC_CHARS: usize = 24_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmGap {
    pub item: String,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct LlmRequest<'a> {
    pub endpoint: &'a str,
    pub model: &'a str,
    pub api_key: Option<&'a str>,
    pub spec: &'a str,
    pub root: &'a Path,
    pub intent: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmOutcome {
    pub gaps: Vec<LlmGap>,
    pub skipped: Option<String>,
}

pub fn resolve_target(endpoint: &str, model: &str) -> (String, String, Option<String>) {
    let key = std::env::var("XAI_API_KEY")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            std::env::var("LLM_API_KEY")
                .ok()
                .filter(|value| !value.is_empty())
        });
    if endpoint == DEFAULT_ENDPOINT {
        if let Some(key) = key.clone() {
            return (
                SPACEXAI_ENDPOINT.to_string(),
                SPACEXAI_MODEL.to_string(),
                Some(key),
            );
        }
    }
    (endpoint.to_string(), model.to_string(), key)
}

pub fn review(request: LlmRequest<'_>) -> LlmOutcome {
    let (endpoint, model, key) = resolve_target(request.endpoint, request.model);
    if endpoint.contains("api.x.ai") && key.is_none() {
        return LlmOutcome {
            gaps: Vec::new(),
            skipped: Some("XAI_API_KEY is not set".into()),
        };
    }
    let spec: String = request.spec.chars().take(MAX_SPEC_CHARS).collect();
    let intent = request.intent.unwrap_or("").trim();
    let user = if intent.is_empty() {
        format!("Spec:\n{spec}")
    } else {
        format!("Intent:\n{intent}\n\nSpec:\n{spec}")
    };
    let mut messages = vec![
        json!({
            "role": "system",
            "content": "You compare a spec with a Rust crate. Return JSON only: {\"gaps\":[{\"item\":\"...\",\"detail\":\"...\"}]}. List spec items the code does not satisfy. The intent is the caller's goal; do not treat a deliberate choice recorded there as a gap. Do not invent CRAP scores or mutation scores. If nothing is missing, return {\"gaps\":[]}."
        }),
        json!({
            "role": "user",
            "content": user
        }),
    ];
    for _ in 0..MAX_TOOL_ROUNDS {
        let body = json!({
            "model": model,
            "messages": messages,
            "max_tokens": MAX_TOKENS,
            "tools": tools(),
        });
        let response = match post_chat(&endpoint, key.as_deref(), &body) {
            Ok(response) => response,
            Err(err) => {
                return LlmOutcome {
                    gaps: Vec::new(),
                    skipped: Some(err),
                };
            }
        };
        let Some(choice) = response.pointer("/choices/0/message") else {
            return LlmOutcome {
                gaps: Vec::new(),
                skipped: Some("llm response has no message".into()),
            };
        };
        if let Some(calls) = choice.get("tool_calls").and_then(Value::as_array) {
            if calls.is_empty() {
                break;
            }
            messages.push(choice.clone());
            for call in calls {
                let id = call.get("id").and_then(Value::as_str).unwrap_or("");
                let function = call.get("function").cloned().unwrap_or(json!({}));
                let name = function.get("name").and_then(Value::as_str).unwrap_or("");
                let arguments = function
                    .get("arguments")
                    .and_then(Value::as_str)
                    .unwrap_or("{}");
                let content = run_tool(request.root, &spec, name, arguments);
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": id,
                    "content": content,
                }));
            }
            continue;
        }
        let content = choice.get("content").and_then(Value::as_str).unwrap_or("");
        return match parse_gaps(content) {
            Some(gaps) => LlmOutcome {
                gaps,
                skipped: None,
            },
            None => LlmOutcome {
                gaps: Vec::new(),
                skipped: Some("llm response was not spec-gap json".into()),
            },
        };
    }
    LlmOutcome {
        gaps: Vec::new(),
        skipped: Some("llm tool round limit reached".into()),
    }
}

pub fn parse_gaps(text: &str) -> Option<Vec<LlmGap>> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    let value: Value = serde_json::from_str(&text[start..=end]).ok()?;
    let gaps = value.get("gaps")?.as_array()?;
    Some(
        gaps.iter()
            .filter_map(|gap| {
                let item = gap.get("item").and_then(Value::as_str)?.to_string();
                let detail = gap
                    .get("detail")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if item.is_empty() {
                    None
                } else {
                    Some(LlmGap { item, detail })
                }
            })
            .collect(),
    )
}

fn post_chat(endpoint: &str, api_key: Option<&str>, body: &Value) -> Result<Value, String> {
    let url = format!("{}/chat/completions", endpoint.trim_end_matches('/'));
    let mut request = ureq::post(&url)
        .set("Content-Type", "application/json")
        .timeout(Duration::from_secs(60));
    if let Some(key) = api_key {
        request = request.set("Authorization", &format!("Bearer {key}"));
    }
    request
        .send_json(body.clone())
        .map_err(|err| err.to_string())?
        .into_json()
        .map_err(|err| err.to_string())
}

fn tools() -> Value {
    json!([
        {"type":"function","function":{"name":"get_file","description":"Read a source file","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}},
        {"type":"function","function":{"name":"get_span","description":"Read lines from a file","parameters":{"type":"object","properties":{"path":{"type":"string"},"start":{"type":"integer"},"end":{"type":"integer"}},"required":["path","start","end"]}}},
        {"type":"function","function":{"name":"callers_of","description":"List files whose text mentions a symbol","parameters":{"type":"object","properties":{"symbol":{"type":"string"}},"required":["symbol"]}}},
        {"type":"function","function":{"name":"tests_covering","description":"List test files that mention a symbol","parameters":{"type":"object","properties":{"symbol":{"type":"string"}},"required":["symbol"]}}},
        {"type":"function","function":{"name":"spec_section","description":"Return the spec text under a heading","parameters":{"type":"object","properties":{"heading":{"type":"string"}},"required":["heading"]}}}
    ])
}

fn run_tool(root: &Path, spec: &str, name: &str, arguments: &str) -> String {
    let args: Value = serde_json::from_str(arguments).unwrap_or(json!({}));
    let result = match name {
        "get_file" => read_capped(root, args.get("path").and_then(Value::as_str).unwrap_or("")),
        "get_span" => read_span(
            root,
            args.get("path").and_then(Value::as_str).unwrap_or(""),
            args.get("start").and_then(Value::as_u64).unwrap_or(1) as usize,
            args.get("end").and_then(Value::as_u64).unwrap_or(1) as usize,
        ),
        "callers_of" => search_symbol(
            root,
            args.get("symbol").and_then(Value::as_str).unwrap_or(""),
            false,
        ),
        "tests_covering" => search_symbol(
            root,
            args.get("symbol").and_then(Value::as_str).unwrap_or(""),
            true,
        ),
        "spec_section" => spec_section(
            spec,
            args.get("heading").and_then(Value::as_str).unwrap_or(""),
        ),
        _ => format!("unknown tool {name}"),
    };
    result.chars().take(4000).collect()
}

fn read_capped(root: &Path, rel: &str) -> String {
    if rel.contains("..") {
        return "path rejected".into();
    }
    std::fs::read_to_string(root.join(rel)).unwrap_or_else(|err| err.to_string())
}

fn read_span(root: &Path, rel: &str, start: usize, end: usize) -> String {
    let text = read_capped(root, rel);
    text.lines()
        .skip(start.saturating_sub(1))
        .take(end.saturating_sub(start).saturating_add(1))
        .collect::<Vec<_>>()
        .join("\n")
}

fn search_symbol(root: &Path, symbol: &str, tests_only: bool) -> String {
    if symbol.is_empty() {
        return "missing symbol".into();
    }
    let mut hits = Vec::new();
    let mut stack = vec![root.join("src"), root.join("tests")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if tests_only && !rel.contains("test") {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                if text.contains(symbol) {
                    hits.push(rel);
                }
            }
        }
    }
    if hits.is_empty() {
        "none".into()
    } else {
        hits.join("\n")
    }
}

fn spec_section(spec: &str, heading: &str) -> String {
    if heading.is_empty() {
        return spec.chars().take(4000).collect();
    }
    let mut capture = false;
    let mut out = String::new();
    for line in spec.lines() {
        if line.starts_with('#') {
            if capture {
                break;
            }
            capture = line
                .to_ascii_lowercase()
                .contains(&heading.to_ascii_lowercase());
        }
        if capture {
            out.push_str(line);
            out.push('\n');
        }
    }
    if out.is_empty() {
        "section not found".into()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gap_json_and_ignores_score_prose() {
        let text = "Here you go {\"gaps\":[{\"item\":\"fn missing\",\"detail\":\"not in src\"}]}";
        let gaps = parse_gaps(text).unwrap();
        assert_eq!(gaps[0].item, "fn missing");
        assert!(parse_gaps("CRAP is 10").is_none());
    }

    #[test]
    fn default_endpoint_uses_spacexai_when_the_key_is_set() {
        std::env::set_var("XAI_API_KEY", "test-key");
        let (endpoint, model, key) = resolve_target(DEFAULT_ENDPOINT, "qwen2.5-coder");
        assert_eq!(endpoint, SPACEXAI_ENDPOINT);
        assert_eq!(model, SPACEXAI_MODEL);
        assert_eq!(key.as_deref(), Some("test-key"));
        std::env::remove_var("XAI_API_KEY");
    }
}

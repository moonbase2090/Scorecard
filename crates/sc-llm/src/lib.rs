// SPDX-License-Identifier: MPL-2.0
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
    resolve_target_with(endpoint, model, key)
}

fn resolve_target_with(
    endpoint: &str,
    model: &str,
    key: Option<String>,
) -> (String, String, Option<String>) {
    if endpoint == DEFAULT_ENDPOINT {
        if let Some(key) = key {
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
    review_with(request, post_chat)
}

/// Opt-in Cursor backend. `cursor-agent` runs in ask mode, which is read-only:
/// the command does not pass `--force` or `--yolo`. The spec and any file the
/// agent reads are sent to Cursor.
pub fn review_cursor(request: LlmRequest<'_>) -> LlmOutcome {
    review_cursor_with(request, run_cursor_agent)
}

pub fn cursor_agent_args(root: &Path, model: &str) -> Vec<String> {
    vec![
        "--print".into(),
        "--mode".into(),
        "ask".into(),
        "--output-format".into(),
        "json".into(),
        "--trust".into(),
        "--workspace".into(),
        root.display().to_string(),
        "--model".into(),
        model.into(),
    ]
}

fn review_cursor_with(
    request: LlmRequest<'_>,
    run: impl Fn(&Path, &[String], &str) -> Result<String, String>,
) -> LlmOutcome {
    let model = request.model.trim();
    if model.is_empty() {
        return LlmOutcome {
            gaps: Vec::new(),
            skipped: Some("cursor backend needs [llm] model set to a cursor-agent model".into()),
        };
    }
    let prompt = cursor_prompt(request.spec, request.intent);
    let args = cursor_agent_args(request.root, model);
    let output = match run(request.root, &args, &prompt) {
        Ok(output) => output,
        Err(err) => {
            return LlmOutcome {
                gaps: Vec::new(),
                skipped: Some(err),
            };
        }
    };
    match gaps_from_cursor_output(&output) {
        Some(gaps) => LlmOutcome {
            gaps,
            skipped: None,
        },
        None => LlmOutcome {
            gaps: Vec::new(),
            skipped: Some("cursor-agent response was not spec-gap json".into()),
        },
    }
}

fn cursor_prompt(spec: &str, intent: Option<&str>) -> String {
    let spec: String = spec.chars().take(MAX_SPEC_CHARS).collect();
    let intent = intent.unwrap_or("").trim();
    let goal = if intent.is_empty() {
        String::new()
    } else {
        format!("Intent:\n{intent}\n\n")
    };
    format!(
        "{goal}Spec:\n{spec}\n\n\
Compare the spec with this repository. Read the files before you claim anything is missing. \
Do not edit files. Do not run commands that change the tree. \
Return JSON only: {{\"gaps\":[{{\"item\":\"...\",\"detail\":\"...\"}}]}}. \
List spec items the code does not satisfy. If nothing is missing, return {{\"gaps\":[]}}."
    )
}

pub fn gaps_from_cursor_output(text: &str) -> Option<Vec<LlmGap>> {
    let trimmed = text.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return gaps_from_cursor_value(&value);
    }
    let mut last = None;
    for line in trimmed.lines() {
        if let Ok(value) = serde_json::from_str::<Value>(line) {
            if value.get("gaps").is_some()
                || value.get("result").is_some()
                || value.get("type").and_then(Value::as_str) == Some("result")
            {
                last = Some(value);
            }
        }
    }
    last.and_then(|value| gaps_from_cursor_value(&value))
}

fn gaps_from_cursor_value(value: &Value) -> Option<Vec<LlmGap>> {
    if value.get("gaps").is_some() {
        return parse_gaps(&value.to_string());
    }
    let result = value.get("result")?;
    if let Some(text) = result.as_str() {
        return parse_gaps(text);
    }
    gaps_from_cursor_value(result)
}

fn run_cursor_agent(root: &Path, args: &[String], prompt: &str) -> Result<String, String> {
    let mut command = std::process::Command::new("cursor-agent");
    command.args(args).arg(prompt).current_dir(root);
    let output = command
        .output()
        .map_err(|err| format!("cursor-agent is not available: {err}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "cursor-agent exited {}: {}",
            output.status,
            stderr.trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|err| err.to_string())
}

fn review_with(
    request: LlmRequest<'_>,
    post: impl Fn(&str, Option<&str>, &Value) -> Result<Value, String>,
) -> LlmOutcome {
    let (endpoint, model, key) = if request.api_key.is_some() {
        (
            request.endpoint.trim_end_matches('/').to_string(),
            request.model.to_string(),
            request.api_key.map(str::to_string),
        )
    } else {
        resolve_target(request.endpoint, request.model)
    };
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
        let response = match post(&endpoint, key.as_deref(), &body) {
            Ok(response) => response,
            Err(err) => {
                return LlmOutcome {
                    gaps: Vec::new(),
                    skipped: Some(redact_secret(&err, key.as_deref())),
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

/// Reads `env_name` and returns the key. The error names the variable and
/// never includes a key value.
pub fn env_api_key(env_name: &str) -> Result<String, String> {
    let name = if env_name.trim().is_empty() {
        "OPENROUTER_API_KEY"
    } else {
        env_name.trim()
    };
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        _ => Err(format!("{name} is not set")),
    }
}

pub fn redact_secret(text: &str, secret: Option<&str>) -> String {
    let Some(secret) = secret.filter(|value| value.len() >= 6) else {
        return text.to_string();
    };
    text.replace(secret, "[redacted]")
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
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn cursor_args_are_read_only_ask_mode() {
        let args = cursor_agent_args(Path::new("/repo"), "gpt-5.3-codex-high");
        assert!(args.windows(2).any(|pair| pair == ["--mode", "ask"]));
        assert!(args.iter().any(|arg| arg == "--print"));
        assert!(!args.iter().any(|arg| arg == "--force" || arg == "--yolo"));
        assert!(args.windows(2).any(|pair| pair == ["--workspace", "/repo"]));
    }

    #[test]
    fn cursor_output_reads_gaps_from_a_result_string() {
        let text = "{\"type\":\"result\",\"result\":\"{\\\"gaps\\\":[{\\\"item\\\":\\\"background images\\\",\\\"detail\\\":\\\"not implemented\\\"}]}\"}";
        let gaps = gaps_from_cursor_output(text).unwrap();
        assert_eq!(gaps[0].item, "background images");
        let lines = "{\"type\":\"result\",\"result\":\"{\\\"gaps\\\":[]}\"}\n";
        assert!(gaps_from_cursor_output(lines).unwrap().is_empty());
    }

    #[test]
    fn missing_env_key_names_the_variable_and_not_a_secret() {
        let name = "SC_TEST_OPENROUTER_ABSENT";
        std::env::remove_var(name);
        let err = env_api_key(name).unwrap_err();
        assert!(err.contains(name));
        assert!(!err.contains("sk-"));
        assert_eq!(
            redact_secret("bearer sk-live-secret failed", Some("sk-live-secret")),
            "bearer [redacted] failed"
        );
    }

    #[test]
    fn explicit_key_stays_on_the_requested_endpoint() {
        let dir = std::env::temp_dir();
        let seen = std::cell::RefCell::new(String::new());
        let outcome = review_with(
            LlmRequest {
                endpoint: "https://openrouter.ai/api/v1",
                model: "x-ai/grok-4",
                api_key: Some("openrouter-secret-value"),
                spec: "Spec item.",
                root: &dir,
                intent: None,
            },
            |endpoint, key, _body| {
                *seen.borrow_mut() = format!("{endpoint}|{}", key.unwrap_or(""));
                Ok(json!({"choices":[{"message":{"content":"{\"gaps\":[]}"}}]}))
            },
        );
        assert!(outcome.skipped.is_none());
        let seen = seen.into_inner();
        assert!(seen.starts_with("https://openrouter.ai/api/v1|openrouter-secret-value"));
        assert!(!seen.contains("api.x.ai"));
    }

    #[test]
    fn parses_gap_json_and_ignores_score_prose() {
        let text = "Here you go {\"gaps\":[{\"item\":\"fn missing\",\"detail\":\"not in src\"}]}";
        let gaps = parse_gaps(text).unwrap();
        assert_eq!(gaps[0].item, "fn missing");
        assert!(parse_gaps("CRAP is 10").is_none());
    }

    #[test]
    fn parses_gap_json_skips_empty_items() {
        let text = "{\"gaps\":[{\"item\":\"\",\"detail\":\"x\"},{\"item\":\"kept\"}]}";
        let gaps = parse_gaps(text).unwrap();
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].item, "kept");
        assert!(parse_gaps("{\"gaps\":{}}").is_none());
    }

    #[test]
    fn default_endpoint_uses_spacexai_when_the_key_is_set() {
        let (endpoint, model, key) =
            resolve_target_with(DEFAULT_ENDPOINT, "qwen2.5-coder", Some("test-key".into()));
        assert_eq!(endpoint, SPACEXAI_ENDPOINT);
        assert_eq!(model, SPACEXAI_MODEL);
        assert_eq!(key.as_deref(), Some("test-key"));
    }

    #[test]
    fn custom_endpoints_pass_through_with_their_key() {
        let (endpoint, _, key) =
            resolve_target_with("http://localhost:8080/v1", "qwen", Some("k".into()));
        assert_eq!(endpoint, "http://localhost:8080/v1");
        assert_eq!(key.as_deref(), Some("k"));
        let (endpoint, _, key) = resolve_target_with(DEFAULT_ENDPOINT, "qwen", None);
        assert_eq!(endpoint, DEFAULT_ENDPOINT);
        assert_eq!(key, None);
    }

    static TREE_SEQ: AtomicU64 = AtomicU64::new(0);

    fn test_tree() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sc-llm-test-{}-{}",
            std::process::id(),
            TREE_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("tests")).unwrap();
        std::fs::write(dir.join("src").join("a.rs"), "fn target() {}\n").unwrap();
        std::fs::write(dir.join("tests").join("t.rs"), "use target;\n").unwrap();
        dir
    }

    fn request<'a>(root: &'a Path, spec: &'a str, endpoint: &'a str) -> LlmRequest<'a> {
        LlmRequest {
            endpoint,
            model: "test-model",
            api_key: None,
            spec,
            root,
            intent: None,
        }
    }

    fn tool_response(calls: Value) -> Value {
        json!({"choices": [{"message": {"tool_calls": calls, "content": null}}]})
    }

    fn content_response(text: &str) -> Value {
        json!({"choices": [{"message": {"content": text}}]})
    }

    #[test]
    fn review_skips_spacexai_without_a_key() {
        let dir = test_tree();
        let outcome = review_with(request(&dir, "spec", SPACEXAI_ENDPOINT), |_, _, _| {
            panic!("must not call the network when the key is missing")
        });
        assert!(outcome.gaps.is_empty());
        assert_eq!(outcome.skipped.as_deref(), Some("XAI_API_KEY is not set"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn review_reports_post_errors() {
        let dir = test_tree();
        let outcome = review_with(request(&dir, "spec", "http://127.0.0.1:1/v1"), |_, _, _| {
            Err("connection refused".to_string())
        });
        assert_eq!(outcome.skipped.as_deref(), Some("connection refused"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn review_needs_a_message_choice() {
        let dir = test_tree();
        let outcome = review_with(request(&dir, "spec", "http://127.0.0.1:1/v1"), |_, _, _| {
            Ok(json!({"choices": []}))
        });
        assert_eq!(
            outcome.skipped.as_deref(),
            Some("llm response has no message")
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn review_runs_a_tool_then_parses_gaps() {
        let dir = test_tree();
        let calls = std::cell::Cell::new(0);
        let outcome = review_with(
            request(&dir, "# Spec\nitem", "http://127.0.0.1:1/v1"),
            |_, _, _| {
                calls.set(calls.get() + 1);
                Ok(if calls.get() == 1 {
                    tool_response(
                        json!([{"id": "c1", "function": {"name": "callers_of", "arguments": "{\"symbol\":\"target\"}"}}]),
                    )
                } else {
                    content_response("prefix {\"gaps\":[{\"item\":\"missing\",\"detail\":\"d\"}]}")
                })
            },
        );
        assert_eq!(outcome.skipped, None);
        assert_eq!(outcome.gaps.len(), 1);
        assert_eq!(outcome.gaps[0].item, "missing");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn review_stops_at_the_tool_round_limit() {
        let dir = test_tree();
        let outcome = review_with(request(&dir, "spec", "http://127.0.0.1:1/v1"), |_, _, _| {
            Ok(tool_response(
                json!([{"id": "c1", "function": {"name": "nope", "arguments": "{}"}}]),
            ))
        });
        assert_eq!(
            outcome.skipped.as_deref(),
            Some("llm tool round limit reached")
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn review_rejects_non_json_content() {
        let dir = test_tree();
        let outcome = review_with(request(&dir, "spec", "http://127.0.0.1:1/v1"), |_, _, _| {
            Ok(content_response("just prose, no json"))
        });
        assert_eq!(
            outcome.skipped.as_deref(),
            Some("llm response was not spec-gap json")
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn search_symbol_filters_tests_and_empty_queries() {
        let dir = test_tree();
        assert_eq!(search_symbol(&dir, "", false), "missing symbol");
        assert_eq!(search_symbol(&dir, "absent", false), "none");
        let all = search_symbol(&dir, "target", false);
        assert!(all.contains("src/a.rs"));
        assert!(all.contains("tests/t.rs"));
        let tests = search_symbol(&dir, "target", true);
        assert!(!tests.contains("src/a.rs"));
        assert!(tests.contains("tests/t.rs"));
        assert_eq!(search_symbol(&dir.join("missing"), "target", false), "none");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn file_tools_reject_bad_paths_and_slice_spans() {
        let dir = test_tree();
        assert_eq!(read_capped(&dir, "../evil.rs"), "path rejected");
        assert!(!read_capped(&dir, "nope.rs").is_empty());
        assert_eq!(read_span(&dir, "src/a.rs", 1, 1), "fn target() {}");
        assert_eq!(read_span(&dir, "../evil.rs", 1, 9), "path rejected");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn spec_section_matches_headings() {
        let spec = "# Alpha\na1\n# Beta\nb1\n";
        assert!(spec_section(spec, "beta").contains("b1"));
        assert!(!spec_section(spec, "beta").contains("a1"));
        assert_eq!(spec_section(spec, "gamma"), "section not found");
        assert!(spec_section(spec, "").contains("# Alpha"));
    }
}

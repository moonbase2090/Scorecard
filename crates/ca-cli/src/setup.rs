//! Install the agent skill and register the `sc-mcp` server.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const SKILL: &str = include_str!("../../../skills/scorecard/SKILL.md");

pub fn run() -> i32 {
    let home = match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home),
        None => {
            eprintln!("HOME is not set");
            return 2;
        }
    };
    let mut lines = Vec::new();
    for rel in [
        ".grok/skills/scorecard",
        ".claude/skills/scorecard",
        ".cursor/skills/scorecard",
        ".agents/skills/scorecard",
    ] {
        let dir = home.join(rel);
        match write_skill(&dir) {
            Ok(path) => lines.push(format!("skill {}", path.display())),
            Err(err) => {
                eprintln!("skill {}: {err}", dir.display());
                return 2;
            }
        }
    }
    match mcp_bin() {
        Some(bin) => {
            if let Err(err) = register_mcp(&home, &bin, &mut lines) {
                eprintln!("mcp: {err}");
                return 2;
            }
        }
        None => lines.push(
            "mcp skipped: sc-mcp is not installed (cargo install --path crates/ca-mcp)".into(),
        ),
    }
    for line in lines {
        println!("{line}");
    }
    0
}

fn write_skill(dir: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    let path = dir.join("SKILL.md");
    fs::write(&path, SKILL).map_err(|err| err.to_string())?;
    Ok(path)
}

fn register_mcp(home: &Path, bin: &Path, lines: &mut Vec<String>) -> Result<(), String> {
    let command = bin.display().to_string();
    let grok = home.join(".grok/config.toml");
    let grok_text = fs::read_to_string(&grok).unwrap_or_default();
    let grok_next = append_grok(&grok_text, &command);
    if grok_next != grok_text {
        if let Some(parent) = grok.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        fs::write(&grok, grok_next).map_err(|err| err.to_string())?;
        lines.push(format!("mcp grok {}", grok.display()));
    } else {
        lines.push(format!("mcp grok already registered {}", grok.display()));
    }

    for (label, path) in [
        ("cursor", home.join(".cursor/mcp.json")),
        ("claude", home.join(".claude.json")),
    ] {
        let status = merge_mcp_json(&path, &command)?;
        lines.push(format!("mcp {label} {status} {}", path.display()));
    }
    Ok(())
}

fn append_grok(text: &str, command: &str) -> String {
    let mut text = text.to_string();
    if text.contains("[mcp_servers.ca]") && text.contains("ca-mcp") {
        text = text.replace("[mcp_servers.ca]", "[mcp_servers.sc]");
    }
    if text.contains("[mcp_servers.sc]") {
        return retarget_toml_command(&text, "[mcp_servers.sc]", command);
    }
    let mut out = text;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("\n[mcp_servers.sc]\ncommand = \"");
    out.push_str(&command.replace('\\', "\\\\").replace('"', "\\\""));
    out.push_str("\"\nenabled = true\n");
    out
}

fn merge_mcp_json(path: &Path, command: &str) -> Result<String, String> {
    if !path.is_file() {
        let body = format!(
            "{{\n  \"mcpServers\": {{\n    \"sc\": {{\"command\": \"{}\", \"args\": []}}\n  }}\n}}\n",
            json_escape(command)
        );
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        fs::write(path, body).map_err(|err| err.to_string())?;
        return Ok("registered".into());
    }
    let original = fs::read_to_string(path).map_err(|err| err.to_string())?;
    let text = retarget_json_command(&rename_legacy_json(&original), command);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|err| format!("{}: {err}", path.display()))?;
    if value
        .get("mcpServers")
        .and_then(|servers| servers.get("sc"))
        .is_some()
    {
        if text != original {
            fs::write(path, &text).map_err(|err| err.to_string())?;
            return Ok("updated".into());
        }
        return Ok("already registered".into());
    }
    let next = insert_mcp_server(&text, command)?;
    fs::write(path, next).map_err(|err| err.to_string())?;
    Ok("registered".into())
}

fn insert_mcp_server(text: &str, command: &str) -> Result<String, String> {
    let entry = format!(
        "\"sc\": {{\"command\": \"{}\", \"args\": []}}",
        json_escape(command)
    );
    if let Some(idx) = root_key(text, "mcpServers") {
        let brace = text[idx..]
            .find('{')
            .map(|offset| idx + offset)
            .ok_or("mcpServers has no object")?;
        let rest = text[brace + 1..].trim_start();
        let inserted = if rest.starts_with('}') {
            format!("\n    {entry}\n  ")
        } else {
            format!("\n    {entry},\n")
        };
        let mut out = String::new();
        out.push_str(&text[..=brace]);
        out.push_str(&inserted);
        out.push_str(&text[brace + 1..]);
        return Ok(out);
    }
    let trimmed = text.trim_end();
    let Some(end) = trimmed.rfind('}') else {
        return Err("config is not a JSON object".into());
    };
    let prefix = &trimmed[..end];
    let body = prefix.trim_end();
    let comma = if body.ends_with('{') { "" } else { "," };
    Ok(format!(
        "{body}{comma}\n  \"mcpServers\": {{\n    {entry}\n  }}\n}}\n"
    ))
}

/// Byte index of a key on the root JSON object. Nested objects are skipped.
fn root_key(text: &str, key: &str) -> Option<usize> {
    let needle = format!("\"{key}\"");
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if in_string {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        match c {
            '"' => {
                if depth == 1 && text[i..].starts_with(&needle) {
                    return Some(i);
                }
                in_string = true;
            }
            '{' | '[' => depth += 1,
            '}' | ']' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    None
}

fn rename_legacy_json(text: &str) -> String {
    let Some(key_at) = root_key(text, "mcpServers") else {
        return text.to_string();
    };
    let Some(rel) = text[key_at..].find('{') else {
        return text.to_string();
    };
    let start = key_at + rel;
    let Some(end) = matching_brace(text, start) else {
        return text.to_string();
    };
    let object = &text[start..=end];
    let Some(pos) = root_key(object, "ca") else {
        return text.to_string();
    };
    let after = &object[pos + 4..];
    if !after.trim_start().starts_with(':') {
        return text.to_string();
    }
    let horizon = &after[..after.len().min(300)];
    if !horizon.contains("ca-mcp") {
        return text.to_string();
    }
    let abs = start + pos;
    let mut out = String::new();
    out.push_str(&text[..abs]);
    out.push_str("\"sc\"");
    out.push_str(&text[abs + 4..]);
    out
}

fn matching_brace(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    let mut i = open;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if in_string {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn json_escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

fn retarget_toml_command(text: &str, header: &str, command: &str) -> String {
    let Some(start) = text.find(header) else {
        return text.to_string();
    };
    let Some(rel) = text[start..].find("command = \"") else {
        return text.to_string();
    };
    let value_at = start + rel + "command = \"".len();
    let Some(end_rel) = text[value_at..].find('"') else {
        return text.to_string();
    };
    let current = &text[value_at..value_at + end_rel];
    if !current.contains("ca-mcp") {
        return text.to_string();
    }
    let mut out = String::new();
    out.push_str(&text[..value_at]);
    out.push_str(&command.replace('\\', "\\\\").replace('"', "\\\""));
    out.push_str(&text[value_at + end_rel..]);
    out
}

fn retarget_json_command(text: &str, command: &str) -> String {
    let Some(key_at) = root_key(text, "mcpServers") else {
        return text.to_string();
    };
    let Some(rel) = text[key_at..].find('{') else {
        return text.to_string();
    };
    let start = key_at + rel;
    let Some(end) = matching_brace(text, start) else {
        return text.to_string();
    };
    let object = &text[start..=end];
    let Some(pos) = root_key(object, "sc") else {
        return text.to_string();
    };
    let after = &object[pos..];
    let Some(cmd_rel) = after.find("\"command\"") else {
        return text.to_string();
    };
    let after_cmd = &after[cmd_rel + "\"command\"".len()..];
    let Some(colon) = after_cmd.find(':') else {
        return text.to_string();
    };
    let after_colon = after_cmd[colon + 1..].trim_start();
    if !after_colon.starts_with('"') {
        return text.to_string();
    }
    let value_rel = after_cmd[colon + 1..].find('"').unwrap();
    let value_at = pos + cmd_rel + "\"command\"".len() + colon + 1 + value_rel + 1;
    let abs = start + value_at;
    let Some(end_rel) = text[abs..].find('"') else {
        return text.to_string();
    };
    if !text[abs..abs + end_rel].contains("ca-mcp") {
        return text.to_string();
    }
    let mut out = String::new();
    out.push_str(&text[..abs]);
    out.push_str(&json_escape(command));
    out.push_str(&text[abs + end_rel..]);
    out
}

fn mcp_bin() -> Option<PathBuf> {
    for name in ["sc-mcp", "ca-mcp"] {
        if let Some(path) = which(name) {
            return Some(path);
        }
        if let Ok(exe) = std::env::current_exe() {
            let sibling = exe.with_file_name(name);
            if sibling.is_file() {
                return Some(sibling);
            }
        }
        if let Some(home) = std::env::var_os("HOME") {
            let cargo = PathBuf::from(home).join(".cargo/bin").join(name);
            if cargo.is_file() {
                return Some(cargo);
            }
        }
    }
    None
}

fn which(name: &str) -> Option<PathBuf> {
    let output = Command::new("sh")
        .args(["-c", &format!("command -v {name}")])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let path = PathBuf::from(text.trim());
    path.is_file().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grok_append_is_idempotent() {
        let once = append_grok("", "/usr/local/bin/sc-mcp");
        assert!(once.contains("[mcp_servers.sc]"));
        assert!(once.contains("command = \"/usr/local/bin/sc-mcp\""));
        assert_eq!(append_grok(&once, "/other"), once);
    }

    #[test]
    fn json_insert_keeps_an_existing_server() {
        let text = "{\n  \"mcpServers\": {\n    \"waypoint\": {\"command\": \"wp\"}\n  }\n}\n";
        let next = insert_mcp_server(text, "/bin/ca-mcp").unwrap();
        let value: serde_json::Value = serde_json::from_str(&next).unwrap();
        assert_eq!(value["mcpServers"]["waypoint"]["command"], "wp");
        assert_eq!(value["mcpServers"]["sc"]["command"], "/bin/ca-mcp");
        assert!(value["mcpServers"]["sc"]["args"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn json_insert_uses_the_root_server_object() {
        let text = r#"{"projects":{"/tmp":{"mcpServers":{"other":{"command":"x"}}}},"mcpServers":{"aws":{"command":"aws"}}}"#;
        let next = insert_mcp_server(text, "/bin/ca-mcp").unwrap();
        let value: serde_json::Value = serde_json::from_str(&next).unwrap();
        assert!(value["projects"]["/tmp"]["mcpServers"].get("sc").is_none());
        assert_eq!(value["mcpServers"]["aws"]["command"], "aws");
        assert_eq!(value["mcpServers"]["sc"]["command"], "/bin/ca-mcp");
    }

    #[test]
    fn json_insert_fills_an_empty_server_object() {
        let text = "{\"mcpServers\": {}}";
        let next = insert_mcp_server(text, "/bin/ca-mcp").unwrap();
        let value: serde_json::Value = serde_json::from_str(&next).unwrap();
        assert_eq!(value["mcpServers"]["sc"]["command"], "/bin/ca-mcp");
    }

    #[test]
    fn legacy_ca_server_is_renamed_only_at_the_root() {
        let text = r#"{"projects":{"/tmp":{"mcpServers":{"ca":{"command":"keep"}}}},"mcpServers":{"ca":{"command":"/bin/ca-mcp","args":[]}}}"#;
        let next = rename_legacy_json(text);
        let value: serde_json::Value = serde_json::from_str(&next).unwrap();
        assert_eq!(
            value["projects"]["/tmp"]["mcpServers"]["ca"]["command"],
            "keep"
        );
        assert!(value["mcpServers"].get("ca").is_none());
        assert_eq!(value["mcpServers"]["sc"]["command"], "/bin/ca-mcp");
        let old = "\n[mcp_servers.ca]\ncommand = \"/bin/ca-mcp\"\nenabled = true\n";
        let renamed = append_grok(old, "/bin/sc-mcp");
        assert!(renamed.contains("[mcp_servers.sc]"));
        assert!(renamed.contains("command = \"/bin/sc-mcp\""));
        assert!(!renamed.contains("ca-mcp"));
        let json = r#"{"mcpServers":{"sc":{"command":"/bin/ca-mcp","args":[]}}}"#;
        let next = retarget_json_command(json, "/bin/sc-mcp");
        let value: serde_json::Value = serde_json::from_str(&next).unwrap();
        assert_eq!(value["mcpServers"]["sc"]["command"], "/bin/sc-mcp");
    }
}

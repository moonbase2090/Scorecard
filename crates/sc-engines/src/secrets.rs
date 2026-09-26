// SPDX-License-Identifier: MPL-2.0
use sc_core::{Finding, Span};

pub fn secrets_in_text(text: &str, rel: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line_no = index as u32 + 1;
        if let Some(rule) = match_line(line) {
            let col = line.find(rule_marker(rule)).unwrap_or(0) as u32 + 1;
            out.push(Finding {
                id: format!("secrets:{rel}:{line_no}"),
                rule: rule.to_string(),
                engine: "secrets".into(),
                severity: "error".into(),
                file: rel.to_string(),
                span: Some(Span {
                    start_line: line_no,
                    start_col: col,
                    end_line: line_no,
                    end_col: col.saturating_add(1),
                }),
                symbol: None,
                message: format!("{rule} on line {line_no}"),
                evidence: serde_json::json!({"rule": rule}),
                suggested_action: Some(
                    "Remove the secret from source and rotate it if it is real".into(),
                ),
                disposition: String::new(),
            });
        }
    }
    out
}

fn rule_marker(rule: &str) -> &'static str {
    match rule {
        "secrets.aws_access_key" => "AKIA",
        "secrets.github_token" => "ghp_",
        "secrets.slack_token" => "xox",
        "secrets.stripe_key" => "sk_live_",
        "secrets.private_key" => "-----BEGIN ",
        _ => "",
    }
}

fn match_line(line: &str) -> Option<&'static str> {
    if is_aws_key(line) {
        return Some("secrets.aws_access_key");
    }
    if is_github_pat(line) || is_ghp(line) {
        return Some("secrets.github_token");
    }
    if is_slack(line) {
        return Some("secrets.slack_token");
    }
    if is_stripe(line) {
        return Some("secrets.stripe_key");
    }
    if is_private_key(line) {
        return Some("secrets.private_key");
    }
    None
}

/// The two PEM markers live on separate lines so this file does not match itself.
fn is_private_key(line: &str) -> bool {
    line.contains(pem_begin()) && line.contains(pem_end())
}

fn pem_begin() -> &'static str {
    "-----BEGIN "
}

fn pem_end() -> &'static str {
    "PRIVATE KEY-----"
}

fn is_aws_key(line: &str) -> bool {
    token_run(line, "AKIA", 16, |c| {
        c.is_ascii_uppercase() || c.is_ascii_digit()
    })
}

fn is_ghp(line: &str) -> bool {
    token_run(line, "ghp_", 36, |c| c.is_ascii_alphanumeric())
}

fn is_github_pat(line: &str) -> bool {
    token_run(line, "github_pat_", 20, |c| {
        c.is_ascii_alphanumeric() || c == '_'
    })
}

fn is_slack(line: &str) -> bool {
    let bytes = line.as_bytes();
    let mut index = 0;
    while index + 4 < bytes.len() {
        if bytes[index..].starts_with(b"xox")
            && matches!(bytes.get(index + 3), Some(b'b' | b'a' | b'p' | b'r' | b's'))
            && bytes.get(index + 4) == Some(&b'-')
        {
            let rest = &line[index + 5..];
            let token: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                .collect();
            if token.len() >= 10 {
                return true;
            }
        }
        index += 1;
    }
    false
}

fn is_stripe(line: &str) -> bool {
    token_run(line, "sk_live_", 16, |c| c.is_ascii_alphanumeric())
}

fn token_run(line: &str, prefix: &str, min_tail: usize, tail: impl Fn(char) -> bool) -> bool {
    let Some(start) = line.find(prefix) else {
        return false;
    };
    let after = &line[start + prefix.len()..];
    let count = after.chars().take_while(|c| tail(*c)).count();
    count >= min_tail
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_a_github_token_and_ignores_the_pattern_text() {
        let token = format!("ghp_{}", "A".repeat(36));
        let text = format!("const KEY: &str = \"{token}\";\nconst NOTE: &str = \"ghp_short\";\n");
        let findings = secrets_in_text(&text, "src/lib.rs");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, "secrets.github_token");
        assert_eq!(findings[0].span.as_ref().unwrap().start_line, 1);
    }

    #[test]
    fn flags_a_private_key_and_ignores_the_detector_source() {
        let text = format!("{}RSA {}\n", super::pem_begin(), super::pem_end());
        let findings = secrets_in_text(&text, "src/lib.rs");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, "secrets.private_key");
        let own = include_str!("secrets.rs");
        assert!(secrets_in_text(own, "crates/sc-engines/src/secrets.rs").is_empty());
    }
}

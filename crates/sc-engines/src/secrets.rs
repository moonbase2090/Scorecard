// SPDX-License-Identifier: MPL-2.0
use sc_core::{Finding, Span};

/// Documented AWS example secret. It is not a live credential.
const AWS_DOCUMENTED_SECRET: &str = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";

pub fn secrets_in_text(text: &str, rel: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line_no = index as u32 + 1;
        let Some((rule, at)) = match_line(line) else {
            continue;
        };
        let col = at as u32 + 1;
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
            message: format!(
                "{} on line {line_no}. Remove it from the tree and rotate it if it was used.",
                secret_name(rule)
            ),
            evidence: serde_json::json!({"rule": rule}),
            suggested_action: Some(
                "Remove the secret from source and rotate it if it is real".into(),
            ),
            disposition: String::new(),
        });
    }
    out
}

fn secret_name(rule: &str) -> &'static str {
    match rule {
        "secrets.aws_access_key" => "AWS access key id",
        "secrets.aws_secret_key" => "AWS secret access key",
        "secrets.github_token" => "GitHub token",
        "secrets.slack_token" => "Slack token",
        "secrets.stripe_key" => "Stripe live key",
        "secrets.private_key" => "private key block",
        _ => "secret",
    }
}

fn match_line(line: &str) -> Option<(&'static str, usize)> {
    if let Some(at) = aws_key_at(line) {
        return Some(("secrets.aws_access_key", at));
    }
    if let Some(at) = aws_secret_at(line) {
        return Some(("secrets.aws_secret_key", at));
    }
    if let Some(at) = github_at(line) {
        return Some(("secrets.github_token", at));
    }
    if let Some(at) = slack_at(line) {
        return Some(("secrets.slack_token", at));
    }
    if let Some(at) = stripe_at(line) {
        return Some(("secrets.stripe_key", at));
    }
    if let Some(at) = private_key_at(line) {
        return Some(("secrets.private_key", at));
    }
    None
}

/// The two PEM markers live on separate lines so this file does not match itself.
/// A line that only names the label is not a key; require base64 material too.
fn private_key_at(line: &str) -> Option<usize> {
    let begin = pem_begin();
    if !(line.contains(begin) && line.contains(pem_end())) {
        return None;
    }
    if !has_pem_material(line) {
        return None;
    }
    line.find(begin)
}

fn has_pem_material(line: &str) -> bool {
    let material: String = line
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '+' || *c == '/' || *c == '=')
        .collect();
    // Strip the short ASCII from the PEM markers themselves.
    let without_markers = material
        .replace("BEGIN", "")
        .replace("PRIVATE", "")
        .replace("KEY", "")
        .replace("RSA", "")
        .replace("EC", "")
        .replace("OPENSSH", "")
        .replace("ENCRYPTED", "");
    without_markers.len() >= 32 && shannon(&without_markers) >= 3.0
}

fn pem_begin() -> &'static str {
    "-----BEGIN "
}

fn pem_end() -> &'static str {
    "PRIVATE KEY-----"
}

fn aws_key_at(line: &str) -> Option<usize> {
    let mut found = None;
    for prefix in ["AKIA", "ASIA"] {
        if let Some(at) = token_at(line, prefix, 16, |c| {
            c.is_ascii_uppercase() || c.is_ascii_digit()
        }) {
            let token = aws_access_token(line, at);
            // AWS docs use ids that end in EXAMPLE (AKIAIOSFODNN7EXAMPLE).
            if token.ends_with("EXAMPLE") {
                continue;
            }
            found = Some(found.map_or(at, |prev: usize| prev.min(at)));
        }
    }
    found
}

fn aws_access_token(line: &str, at: usize) -> &str {
    let bytes = line.as_bytes();
    let mut end = at;
    while end < bytes.len() && (bytes[end].is_ascii_uppercase() || bytes[end].is_ascii_digit()) {
        end += 1;
    }
    &line[at..end]
}

fn github_at(line: &str) -> Option<usize> {
    let mut found = None;
    for prefix in ["github_pat_", "ghp_", "gho_", "ghu_", "ghs_", "ghr_"] {
        let min_tail = if prefix == "github_pat_" { 20 } else { 36 };
        let allow_underscore = prefix == "github_pat_";
        if let Some(at) = token_at(line, prefix, min_tail, |c| {
            c.is_ascii_alphanumeric() || (allow_underscore && c == '_')
        }) {
            let token = token_body(line, at, prefix.len(), |c| {
                c.is_ascii_alphanumeric() || (allow_underscore && c == '_')
            });
            if !credential_signal(token) {
                continue;
            }
            found = Some(found.map_or(at, |prev: usize| prev.min(at)));
        }
    }
    found
}

/// A 40-character secret next to an AWS secret-key assignment.
/// The documented example and a low-entropy string are not keys.
fn aws_secret_at(line: &str) -> Option<usize> {
    let lower = line.to_ascii_lowercase();
    if !lower.contains("secret_access_key") && !lower.contains("secretaccesskey") {
        return None;
    }
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if !is_secret_char(bytes[index]) {
            index += 1;
            continue;
        }
        let end = secret_run_end(bytes, index);
        let token = &line[index..end];
        if token.len() == 40 && token != AWS_DOCUMENTED_SECRET && credential_signal(token) {
            return Some(index);
        }
        index = end;
    }
    None
}

fn secret_run_end(bytes: &[u8], start: usize) -> usize {
    let mut index = start;
    while index < bytes.len() && is_secret_char(bytes[index]) {
        index += 1;
    }
    index
}

fn is_secret_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'/' || byte == b'+' || byte == b'='
}

fn shannon(token: &str) -> f64 {
    if token.is_empty() {
        return 0.0;
    }
    let mut counts = [0u32; 128];
    for byte in token.bytes() {
        counts[byte as usize] += 1;
    }
    let total = token.len() as f64;
    let mut entropy = 0.0;
    for count in counts {
        if count == 0 {
            continue;
        }
        let p = f64::from(count) / total;
        entropy -= p * p.log2();
    }
    entropy
}

/// Placeholders and docs examples: low entropy, one repeated character,
/// nearly sorted / sequential bodies, or an obvious filler word.
fn credential_signal(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    let lower = token.to_ascii_lowercase();
    if lower.contains("example")
        || lower.contains("placeholder")
        || lower.contains("000000")
        || lower.contains("aaaaaa")
        || lower.contains("xxxxxx")
    {
        return false;
    }
    if shannon(token) < 3.0 {
        return false;
    }
    if mostly_sequential(token) {
        return false;
    }
    true
}

fn mostly_sequential(token: &str) -> bool {
    let chars: Vec<u32> = token.chars().map(|c| c as u32).collect();
    if chars.len() < 12 {
        return false;
    }
    let pairs = chars.len() - 1;
    let ascending = chars.windows(2).filter(|w| w[0] <= w[1]).count();
    let descending = chars.windows(2).filter(|w| w[0] >= w[1]).count();
    ascending * 100 / pairs >= 85 || descending * 100 / pairs >= 85
}

fn slack_at(line: &str) -> Option<usize> {
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
            if token.len() >= 10 && credential_signal(&token) {
                return Some(index);
            }
        }
        index += 1;
    }
    None
}

fn stripe_at(line: &str) -> Option<usize> {
    let at = token_at(line, "sk_live_", 16, |c| c.is_ascii_alphanumeric())?;
    let body = token_body(line, at, "sk_live_".len(), |c| c.is_ascii_alphanumeric());
    credential_signal(body).then_some(at)
}

fn token_body(line: &str, at: usize, prefix_len: usize, tail: impl Fn(char) -> bool) -> &str {
    let after = &line[at + prefix_len..];
    let count = after.chars().take_while(|c| tail(*c)).count();
    &after[..count]
}

fn token_at(
    line: &str,
    prefix: &str,
    min_tail: usize,
    tail: impl Fn(char) -> bool,
) -> Option<usize> {
    let mut rest = line;
    let mut offset = 0;
    while let Some(start) = rest.find(prefix) {
        let after = &rest[start + prefix.len()..];
        let count = after.chars().take_while(|c| tail(*c)).count();
        if count >= min_tail {
            return Some(offset + start);
        }
        let next = start + prefix.len();
        offset += next;
        rest = &rest[next..];
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mixed_tail(len: usize) -> String {
        let parts = ["Ab", "3k", "Qm", "9Z", "nR", "4p", "Lx", "7w"];
        parts.iter().cycle().take(len / 2).copied().collect()
    }

    #[test]
    fn flags_a_github_token_and_ignores_the_pattern_text() {
        let token = format!("ghp_{}", mixed_tail(36));
        let text = format!("const KEY: &str = \"{token}\";\nconst NOTE: &str = \"ghp_short\";\n");
        let findings = secrets_in_text(&text, "src/lib.rs");
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].rule, "secrets.github_token");
        assert_eq!(findings[0].span.as_ref().unwrap().start_line, 1);
    }

    #[test]
    fn ignores_documentation_placeholders_and_test_tokens() {
        let cases = [
            "// The AWS docs example access key is AKIAIOSFODNN7EXAMPLE.\n",
            "/// The PEM label is -----BEGIN RSA PRIVATE KEY-----\n",
            "// placeholder ghp_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n",
            "// Slack docs-style placeholder: xoxb-000000000000-placeholder\n",
            "// placeholder sk_live_0000000000000000\n",
            "ghp_1234567890abcdefghijklmnopqrstuvwxyzAB\n",
            "github_pat_11ABCDEFGHIJKLMNOPQRSTUVWXYZ_abcdefghijklmnopqrstuv\n",
        ];
        for text in cases {
            let findings = secrets_in_text(text, "src/lib.rs");
            assert!(
                findings.is_empty(),
                "should ignore placeholder in {text:?}, got {findings:?}"
            );
        }
    }

    #[test]
    fn flags_github_oauth_and_app_tokens() {
        for prefix in ["gho_", "ghu_", "ghs_", "ghr_"] {
            let token = format!("{prefix}{}", mixed_tail(36));
            let text = format!("TOKEN = \"{token}\"\n");
            let findings = secrets_in_text(&text, "app.py");
            assert_eq!(findings.len(), 1, "{prefix} {findings:?}");
            assert_eq!(findings[0].rule, "secrets.github_token");
            assert_eq!(findings[0].span.as_ref().unwrap().start_col, 10);
            let short = format!("{prefix}{}", mixed_tail(34));
            assert!(
                secrets_in_text(&format!("TOKEN = \"{short}\"\n"), "app.py").is_empty(),
                "{prefix} short tail must not match"
            );
        }
    }

    #[test]
    fn flags_a_temporary_aws_access_key_the_same_as_a_long_lived_one() {
        for prefix in ["AKIA", "ASIA"] {
            let id = format!("{prefix}{}", "A1B2C3D4E5F6G7H8");
            assert_eq!(id.len(), 20, "{prefix}");
            let text = format!("aws_access_key_id = \"{id}\"\n");
            let findings = secrets_in_text(&text, "config.env");
            assert_eq!(findings.len(), 1, "{prefix} {findings:?}");
            assert_eq!(findings[0].rule, "secrets.aws_access_key");
            assert!(findings[0].message.contains("AWS access key id"));
        }
        let short = format!("ASIA{}", "A".repeat(15));
        assert!(secrets_in_text(&format!("key = \"{short}\"\n"), "a.env").is_empty());
        let example = "aws_access_key_id = \"AKIAIOSFODNN7EXAMPLE\"\n";
        assert!(
            secrets_in_text(example, "a.env").is_empty(),
            "AWS docs EXAMPLE id must not fail the build"
        );
    }

    #[test]
    fn flags_an_aws_secret_access_key_and_ignores_the_example_and_low_entropy() {
        let key = sample_secret();
        assert_eq!(key.len(), 40);
        assert!(shannon(&key) >= 3.0);
        let text = format!("aws_secret_access_key = \"{key}\"\n");
        let findings = secrets_in_text(&text, "app.py");
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].rule, "secrets.aws_secret_key");
        assert!(findings[0].message.contains("AWS secret access key"));
        assert!(findings[0].message.contains("rotate"));

        let example = format!("aws_secret_access_key = \"{AWS_DOCUMENTED_SECRET}\"\n");
        assert!(
            secrets_in_text(&example, "app.py").is_empty(),
            "documented example must not fail the build"
        );
        let flat = format!("secret_access_key = \"{}\"\n", "A".repeat(40));
        assert!(secrets_in_text(&flat, "app.py").is_empty());
        let bare = format!("value = \"{key}\"\n");
        assert!(
            secrets_in_text(&bare, "app.py").is_empty(),
            "a 40-character value with no secret-key assignment is not a finding"
        );
    }

    fn sample_secret() -> String {
        let parts = ["Ab", "3/", "Kq", "9Z", "mN", "4+", "pL", "7x"];
        parts.iter().cycle().take(20).copied().collect()
    }

    #[test]
    fn flags_a_private_key_and_ignores_the_detector_source() {
        let material = mixed_tail(64);
        let text = format!(
            "{}RSA {} {material}\n",
            super::pem_begin(),
            super::pem_end()
        );
        let findings = secrets_in_text(&text, "src/lib.rs");
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].rule, "secrets.private_key");
        let label_only = format!("{}RSA {}\n", super::pem_begin(), super::pem_end());
        assert!(
            secrets_in_text(&label_only, "src/lib.rs").is_empty(),
            "PEM label without key material must not fail"
        );
        let own = include_str!("secrets.rs");
        assert!(
            secrets_in_text(own, "crates/sc-engines/src/secrets.rs").is_empty(),
            "{:?}",
            secrets_in_text(own, "crates/sc-engines/src/secrets.rs")
        );
    }
}

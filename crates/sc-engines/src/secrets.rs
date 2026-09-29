// SPDX-License-Identifier: MPL-2.0
use sc_core::{Finding, Span};

/// Documented AWS example secret. It is not a live credential.
const AWS_DOCUMENTED_SECRET: &str = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";

pub fn secrets_in_text(text: &str, rel: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    for (index, at) in pem_private_key_hits(&lines) {
        let line_no = index as u32 + 1;
        let col = at as u32 + 1;
        out.push(Finding {
            id: format!("secrets:{rel}:{line_no}"),
            rule: "secrets.private_key".into(),
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
                "private key block on line {line_no}. Remove it from the tree and rotate it if it was used."
            ),
            evidence: serde_json::json!({"rule": "secrets.private_key"}),
            suggested_action: Some(
                "Remove the secret from source and rotate it if it is real".into(),
            ),
            disposition: String::new(),
        });
    }
    for (index, line) in lines.iter().enumerate() {
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
    None
}

/// A PEM private key is a BEGIN header that names `PRIVATE KEY`, plus key
/// material — either on the same line or on following base64 lines before END.
/// A line that only names the label is not a key.
fn pem_private_key_hits(lines: &[&str]) -> Vec<(usize, usize)> {
    let begin = pem_begin();
    let mut hits = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let Some(at) = line.find(begin) else {
            continue;
        };
        if !line.contains(pem_end()) {
            continue;
        }
        if has_inline_pem_material(line) || pem_block_has_body(lines, index) {
            hits.push((index, at));
        }
    }
    hits
}

fn has_inline_pem_material(line: &str) -> bool {
    let material: String = line
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '+' || *c == '/' || *c == '=')
        .collect();
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

fn pem_block_has_body(lines: &[&str], header_idx: usize) -> bool {
    let mut material = String::new();
    for line in lines.iter().skip(header_idx + 1).take(64) {
        let payload = pem_line_payload(line);
        if payload.starts_with("-----END ") && payload.contains(pem_end()) {
            break;
        }
        if payload.starts_with("-----") {
            break;
        }
        if payload.len() >= 16
            && payload
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
        {
            material.push_str(&payload);
        }
    }
    material.len() >= 32 && shannon(&material) >= 3.0
}

/// Drop common source-language wrappers from a PEM body line.
fn pem_line_payload(line: &str) -> String {
    let mut s = line.trim();

    if let Some(item) = s.strip_prefix("- ") {
        s = item.trim_start();
    }
    if let Some(rest) = s.strip_prefix('+') {
        let candidate = rest.trim_start();
        let quoted = candidate.starts_with('"')
            || candidate.starts_with('\'')
            || candidate.starts_with('\x60')
            || ["br", "rb", "fr", "rf", "b", "B", "r", "R", "f", "F"]
                .iter()
                .any(|prefix| {
                    candidate
                        .strip_prefix(prefix)
                        .is_some_and(|value| value.starts_with('"') || value.starts_with('\''))
                });
        if quoted {
            s = candidate;
        }
    }
    if let Some(stripped) = s.strip_suffix(',') {
        s = stripped.trim_end();
    }

    if let Some(before_operator) = s.strip_suffix('+') {
        let before_operator = before_operator.trim_end();
        if before_operator.ends_with('"')
            || before_operator.ends_with('\'')
            || before_operator.ends_with('\x60')
        {
            s = before_operator;
        }
    }

    for prefix in ["br", "rb", "fr", "rf", "b", "B", "r", "R", "f", "F"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            if rest.starts_with('"') || rest.starts_with('\'') {
                s = rest;
                break;
            }
        }
    }

    for delimiter in ["'''", "\"\"\"", "\x60", "\"", "'"] {
        if s.starts_with(delimiter) && s.ends_with(delimiter) && s.len() > delimiter.len() * 2 {
            s = &s[delimiter.len()..s.len() - delimiter.len()];
            break;
        }
    }
    if let Some(stripped) = s.strip_suffix("\\r\\n") {
        s = stripped;
    }
    if let Some(stripped) = s.strip_suffix("\\n") {
        s = stripped;
    }
    if let Some(stripped) = s.strip_suffix("\\r") {
        s = stripped;
    }
    s.trim().to_string()
}

/// The two PEM markers live on separate lines so this file does not match itself.
fn pem_begin() -> &'static str {
    "-----BEGIN "
}

fn pem_end() -> &'static str {
    "PRIVATE KEY-----"
}

fn aws_key_at(line: &str) -> Option<usize> {
    let mut found = None;
    for prefix in ["AKIA", "ASIA"] {
        for at in token_ats(line, prefix, 16, |c| {
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
        for at in token_ats(line, prefix, min_tail, |c| {
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
/// nearly sorted / sequential bodies, or a token whose only letters are a
/// filler word (`placeholder` / `example`). A mixed body that merely embeds
/// that word still has credential signal.
fn credential_signal(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    if shannon(token) < 3.0 {
        return false;
    }
    if mostly_sequential(token) {
        return false;
    }
    let core: String = token
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    if core == "placeholder" || core == "example" {
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
    for at in token_ats(line, "sk_live_", 16, |c| c.is_ascii_alphanumeric()) {
        let body = token_body(line, at, "sk_live_".len(), |c| c.is_ascii_alphanumeric());
        if credential_signal(body) {
            return Some(at);
        }
    }
    None
}

fn token_body(line: &str, at: usize, prefix_len: usize, tail: impl Fn(char) -> bool) -> &str {
    let after = &line[at + prefix_len..];
    let count = after.chars().take_while(|c| tail(*c)).count();
    &after[..count]
}

/// Every offset where `prefix` is followed by at least `min_tail` tail
/// characters. Callers that reject a candidate (a placeholder, or no
/// credential signal) keep looking instead of stopping at the first one,
// so a placeholder earlier on the line cannot hide a real key later on it.
fn token_ats<'a>(
    line: &'a str,
    prefix: &'a str,
    min_tail: usize,
    tail: impl Fn(char) -> bool + 'a,
) -> impl Iterator<Item = usize> + 'a {
    let mut rest = line;
    let mut offset = 0;
    let mut done = false;
    std::iter::from_fn(move || {
        if done {
            return None;
        }
        while let Some(start) = rest.find(prefix) {
            let after = &rest[start + prefix.len()..];
            let count = after.chars().take_while(|c| tail(*c)).count();
            let at = offset + start;
            let next = start + prefix.len();
            offset += next;
            rest = &rest[next..];
            if count >= min_tail {
                return Some(at);
            }
        }
        done = true;
        None
    })
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
    fn a_placeholder_earlier_on_a_line_does_not_hide_a_real_key() {
        // Placeholder first, real key second on the same line: each key is
        // caught when alone, so the placeholder must not excuse the key.
        let aws = format!("{}{}", "AKIA", "QWERTYUIOPASDFGH");
        let ghp_body = ["k7Qm9", "Lx4wAb3Z", "nR8pY2cF9wQx", "T6vB1nM5sDe"].concat();
        let stripe_body = ["51Hq8vN2mK", "x7pL4wZr9T", "cYbQ3aF6d"].concat();
        let cases = [
            (
                format!("# aws: replace AKIAIOSFODNN7EXAMPLE with {aws}\n"),
                "secrets.aws_access_key",
            ),
            (
                format!("# token ghp_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA real ghp_{ghp_body}\n"),
                "secrets.github_token",
            ),
            (
                format!("# stripe sk_live_0000000000000000 real sk_live_{stripe_body}\n"),
                "secrets.stripe_key",
            ),
        ];
        for (text, rule) in cases {
            let findings = secrets_in_text(&text, "notes.txt");
            assert_eq!(findings.len(), 1, "{text:?} got {findings:?}");
            assert_eq!(findings[0].rule, rule);
        }
    }

    #[test]
    fn does_not_ignore_a_mixed_token_that_embeds_zero_runs() {
        // Build the body in pieces so this source file does not match itself.
        let body = ["Ab3k", "000000", "Qm9ZnR4pLx7w", "Ab3k", "Qm9ZnR4pLx"].concat();
        assert_eq!(body.len(), 36, "{body}");
        let token = format!("ghp_{body}");
        let findings = secrets_in_text(&format!("TOKEN = \"{token}\"\n"), "app.py");
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].rule, "secrets.github_token");
    }

    #[test]
    fn does_not_ignore_a_mixed_token_that_embeds_placeholder() {
        // 36-char mixed body that contains the substring "placeholder".
        let body = ["k7Qm9", "placeholder", "Lx4wAb3ZnR8pY2cF9wQx"].concat();
        assert_eq!(body.len(), 36, "{body}");
        assert!(body.contains("placeholder"));
        let token = format!("ghp_{body}");
        let findings = secrets_in_text(&format!("TOKEN = \"{token}\"\n"), "app.py");
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].rule, "secrets.github_token");
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
        let inline = format!(
            "{}RSA {} {material}\n",
            super::pem_begin(),
            super::pem_end()
        );
        let findings = secrets_in_text(&inline, "src/lib.rs");
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].rule, "secrets.private_key");

        let body = [
            "MIIE", "owIB", "AAKC", "AQEA", "0Z3V", "S5J4", "Ab3k", "Qm9Z",
        ]
        .concat();
        let multiline = format!(
            "{}RSA {}\n{body}\n-----END RSA {}\n",
            super::pem_begin(),
            super::pem_end(),
            super::pem_end()
        );
        let findings = secrets_in_text(&multiline, "key.pem");
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].rule, "secrets.private_key");
        assert_eq!(findings[0].span.as_ref().unwrap().start_line, 1);

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

    #[test]
    fn flags_a_normal_multiline_pem_private_key() {
        let body = ["MIIEowIBAAKCAQEA0Z3VS5J4", "Ab3kQm9ZnR4pLx7wKq9ZmN4p"].concat();
        assert!(body.len() >= 32);
        let text = format!(
            "{}RSA {}\n{body}\n-----END RSA {}\n",
            super::pem_begin(),
            super::pem_end(),
            super::pem_end()
        );
        let findings = secrets_in_text(&text, "id_rsa");
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].rule, "secrets.private_key");
        assert_eq!(findings[0].span.as_ref().unwrap().start_line, 1);
    }

    #[test]
    fn flags_pem_written_as_python_string_literals() {
        let body = ["MIIEowIBAAKCAQEA0Z3VS5J4", "Ab3kQm9ZnR4pLx7wKq9ZmN4p"].concat();
        assert!(body.len() >= 32);
        let mid = body.len() / 2;
        let text = format!(
            "KEY = (\n    \"{}RSA {}\\n\"\n    \"{}\\n\"\n    \"{}\\n\"\n    \"-----END RSA {}\\n\"\n)\n",
            super::pem_begin(),
            super::pem_end(),
            &body[..mid],
            &body[mid..],
            super::pem_end()
        );
        let findings = secrets_in_text(&text, "src/config.py");
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].rule, "secrets.private_key");
        assert_eq!(findings[0].span.as_ref().unwrap().start_line, 2);
    }

    #[test]
    fn flags_pem_written_as_javascript_string_array() {
        let body = ["MIIEowIBAAKCAQEA0Z3VS5J4", "Ab3kQm9ZnR4pLx7wKq9ZmN4p"].concat();
        assert!(body.len() >= 32);
        let mid = body.len() / 2;
        let text = format!(
            "const key = [\n  '{}{}',\n  '{}',\n  '{}',\n  '-----END {}',\n].join('\\n');\n",
            super::pem_begin(),
            super::pem_end(),
            &body[..mid],
            &body[mid..],
            super::pem_end()
        );
        let findings = secrets_in_text(&text, "src/key.js");
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].rule, "secrets.private_key");
        assert_eq!(findings[0].span.as_ref().unwrap().start_line, 2);
    }

    #[test]
    fn flags_pem_written_as_go_string_concatenation() {
        let body = ["MIIEowIBAAKCAQEA0Z3VS5J4", "Ab3kQm9ZnR4pLx7wKq9ZmN4p"].concat();
        let mid = body.len() / 2;
        let text = format!(
            "var key = \"{}RSA {}\\n\" +\n    \"{}\\n\" +\n    \"{}\\n\" +\n    \"-----END RSA {}\\n\"\n",
            super::pem_begin(),
            super::pem_end(),
            &body[..mid],
            &body[mid..],
            super::pem_end()
        );
        let findings = secrets_in_text(&text, "src/key.go");
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].rule, "secrets.private_key");
    }

    #[test]
    fn flags_pem_written_as_java_string_concatenation() {
        let body = ["MIIEowIBAAKCAQEA0Z3VS5J4", "Ab3kQm9ZnR4pLx7wKq9ZmN4p"].concat();
        let mid = body.len() / 2;
        let text = format!(
            "String key = \"{}RSA {}\\n\" +\n    + \"{}\\n\" +\n    + \"{}\\n\" +\n    + \"-----END RSA {}\\n\";\n",
            super::pem_begin(),
            super::pem_end(),
            &body[..mid],
            &body[mid..],
            super::pem_end()
        );
        let findings = secrets_in_text(&text, "src/Key.java");
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].rule, "secrets.private_key");
    }

    #[test]
    fn flags_pem_written_with_backtick_literals() {
        let body = ["MIIEowIBAAKCAQEA0Z3VS5J4", "Ab3kQm9ZnR4pLx7wKq9ZmN4p"].concat();
        let mid = body.len() / 2;
        let text = format!(
            "const key = [\n  \x60{}RSA {}\x60,\n  \x60{}\x60,\n  \x60{}\x60,\n  \x60-----END {}\x60,\n];\n",
            super::pem_begin(),
            super::pem_end(),
            &body[..mid],
            &body[mid..],
            super::pem_end()
        );
        let findings = secrets_in_text(&text, "src/key.js");
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].rule, "secrets.private_key");
    }

    #[test]
    fn flags_pem_written_with_python_string_prefixes() {
        let body = ["MIIEowIBAAKCAQEA0Z3VS5J4", "Ab3kQm9ZnR4pLx7wKq9ZmN4p"].concat();
        let mid = body.len() / 2;
        for prefix in ["b", "r", "f"] {
            let text = format!(
                "key = (\n    {prefix}\"{}RSA {}\\n\"\n    {prefix}\"{}\\n\"\n    {prefix}\"{}\\n\"\n    {prefix}\"-----END RSA {}\\n\"\n)\n",
                super::pem_begin(),
                super::pem_end(),
                &body[..mid],
                &body[mid..],
                super::pem_end()
            );
            let findings = secrets_in_text(&text, "src/key.py");
            assert_eq!(findings.len(), 1, "prefix {prefix}: {findings:?}");
            assert_eq!(findings[0].rule, "secrets.private_key");
        }
    }

    #[test]
    fn flags_pem_written_as_yaml_list_items() {
        let body = ["MIIEowIBAAKCAQEA0Z3VS5J4", "Ab3kQm9ZnR4pLx7wKq9ZmN4p"].concat();
        let mid = body.len() / 2;
        let variants = [
            format!(
                "private_key:\n  - \"{}RSA {}\"\n  - \"{}\"\n  - {}\n  - \"-----END RSA {}\"\n",
                super::pem_begin(),
                super::pem_end(),
                &body[..mid],
                &body[mid..],
                super::pem_end()
            ),
            format!(
                "private_key:\n  - {}RSA {}\n  - {}\n  - {}\n  - -----END RSA {}\n",
                super::pem_begin(),
                super::pem_end(),
                &body[..mid],
                &body[mid..],
                super::pem_end()
            ),
        ];
        for text in variants {
            let findings = secrets_in_text(&text, "config/keys.yml");
            assert_eq!(findings.len(), 1, "{findings:?}");
            assert_eq!(findings[0].rule, "secrets.private_key");
        }
    }

    #[test]
    fn flags_pem_written_as_per_line_triple_quoted_literals() {
        let body = ["MIIEowIBAAKCAQEA0Z3VS5J4", "Ab3kQm9ZnR4pLx7wKq9ZmN4p"].concat();
        let mid = body.len() / 2;
        let text = format!(
            "key = (\n    \"\"\"{}RSA {}\"\"\",\n    \"\"\"{}\"\"\",\n    \"\"\"{}\"\"\",\n    \"\"\"-----END RSA {}\"\"\",\n)\n",
            super::pem_begin(),
            super::pem_end(),
            &body[..mid],
            &body[mid..],
            super::pem_end()
        );
        let findings = secrets_in_text(&text, "src/key.py");
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].rule, "secrets.private_key");
    }
}

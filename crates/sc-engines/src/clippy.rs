// SPDX-License-Identifier: MPL-2.0
//! Clippy command scoping and diagnostic parsing.

use std::path::Path;

/// Include every Rust workspace member only when the analyzed directory is the root.
pub(super) fn clippy_workspace(script: &str, workspace_root: bool) -> String {
    let tokens = shell_tokens(script);
    let mut edits = Vec::new();
    let mut command = Vec::new();
    for token in &tokens {
        if token.separator {
            edit_clippy_command(&command, workspace_root, &mut edits);
            command.clear();
        } else {
            command.push(token);
        }
    }
    edit_clippy_command(&command, workspace_root, &mut edits);
    edits.sort_by_key(|(start, _, _)| std::cmp::Reverse(*start));
    let mut rewritten = script.to_string();
    for (start, end, replacement) in edits {
        rewritten.replace_range(start..end, &replacement);
    }
    rewritten
}

struct ShellToken {
    value: String,
    end: usize,
    separator: bool,
}

impl ShellToken {
    fn separator() -> Self {
        Self {
            value: String::new(),
            end: 0,
            separator: true,
        }
    }
}

fn shell_tokens(script: &str) -> Vec<ShellToken> {
    let mut tokens = Vec::new();
    let mut cursor = 0;
    while cursor < script.len() {
        let ch = script[cursor..].chars().next().unwrap();
        if ch.is_whitespace() {
            if ch == '\n' || ch == '\r' {
                tokens.push(ShellToken::separator());
            }
            cursor += ch.len_utf8();
            continue;
        }
        if ch == '#' {
            while cursor < script.len() && !script[cursor..].starts_with('\n') {
                cursor += script[cursor..].chars().next().unwrap().len_utf8();
            }
            continue;
        }
        if matches!(ch, ';' | '|' | '&' | '(' | ')' | '{' | '}') {
            let next = script[cursor + ch.len_utf8()..].chars().next();
            cursor += ch.len_utf8();
            if matches!((ch, next), ('&', Some('&')) | ('|', Some('|'))) {
                cursor += 1;
            }
            tokens.push(ShellToken::separator());
            continue;
        }

        let mut value = String::new();
        let mut quote = None;
        while cursor < script.len() {
            let ch = script[cursor..].chars().next().unwrap();
            if let Some(active_quote) = quote {
                if ch == active_quote {
                    quote = None;
                    cursor += ch.len_utf8();
                } else if ch == '\\' && active_quote == '"' {
                    cursor += ch.len_utf8();
                    if cursor < script.len() {
                        let escaped = script[cursor..].chars().next().unwrap();
                        value.push(escaped);
                        cursor += escaped.len_utf8();
                    }
                } else {
                    value.push(ch);
                    cursor += ch.len_utf8();
                }
                continue;
            }
            if ch.is_whitespace() || matches!(ch, ';' | '|' | '&' | '(' | ')' | '{' | '}') {
                break;
            }
            if ch == '\'' || ch == '"' {
                quote = Some(ch);
                cursor += ch.len_utf8();
            } else if ch == '\\' {
                cursor += ch.len_utf8();
                if cursor < script.len() {
                    let escaped = script[cursor..].chars().next().unwrap();
                    if escaped != '\n' {
                        value.push(escaped);
                    }
                    cursor += escaped.len_utf8();
                }
            } else {
                value.push(ch);
                cursor += ch.len_utf8();
            }
        }
        tokens.push(ShellToken {
            value,
            end: cursor,
            separator: false,
        });
    }
    tokens
}

fn edit_clippy_command(
    command: &[&ShellToken],
    workspace_root: bool,
    edits: &mut Vec<(usize, usize, String)>,
) {
    let mut index = 0;
    while command.get(index).is_some_and(|token| {
        matches!(
            token.value.as_str(),
            "if" | "then" | "elif" | "else" | "while" | "until" | "do" | "!"
        )
    }) {
        index += 1;
    }
    while command
        .get(index)
        .is_some_and(|token| is_env_assignment(&token.value))
    {
        index += 1;
    }
    if command.get(index).is_some_and(|token| token.value == "env") {
        index += 1;
        while command
            .get(index)
            .is_some_and(|token| is_env_assignment(&token.value))
        {
            index += 1;
        }
    }
    if !command
        .get(index)
        .is_some_and(|token| token.value == "cargo")
    {
        return;
    }
    index += 1;
    if command
        .get(index)
        .is_some_and(|token| token.value.starts_with('+'))
    {
        index += 1;
    }
    let Some(clippy) = command.get(index).filter(|token| token.value == "clippy") else {
        return;
    };
    let args = &command[index + 1..];
    let before_rustc_args = args
        .iter()
        .position(|token| token.value == "--")
        .unwrap_or(args.len());
    let workspace_args = &args[..before_rustc_args];
    if workspace_root {
        if !workspace_args
            .iter()
            .any(|token| token.value == "--workspace")
        {
            edits.push((clippy.end, clippy.end, " --workspace".into()));
        }
    } else {
        for (offset, token) in workspace_args.iter().enumerate() {
            if token.value == "--workspace" {
                let previous_end = if offset == 0 {
                    clippy.end
                } else {
                    workspace_args[offset - 1].end
                };
                edits.push((previous_end, token.end, String::new()));
            }
        }
    }
}

fn is_env_assignment(value: &str) -> bool {
    value.split_once('=').is_some_and(|(name, _)| {
        let mut chars = name.chars();
        chars
            .next()
            .is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic())
            && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
    })
}

pub(super) struct ClippyDiagnostic {
    pub(super) file: String,
    pub(super) line: u32,
    pub(super) column: u32,
    pub(super) lint: String,
    pub(super) message: String,
}

pub(super) fn first_clippy_diagnostic(root: &Path, output: &str) -> Option<ClippyDiagnostic> {
    let lines: Vec<_> = output.lines().collect();
    let mut first_warning = None;
    for (index, line) in lines.iter().enumerate() {
        let Some(message) = lint_diagnostic_message(line) else {
            continue;
        };
        let is_error = line.trim_start().starts_with("error: ");
        let mut location = None;
        let mut lint = None;
        for line in lines.iter().skip(index + 1) {
            if lint_diagnostic_message(line).is_some() {
                break;
            }
            if location.is_none() {
                location = diagnostic_location(line);
            }
            if lint.is_none() {
                lint = clippy_lint_name(line);
            }
        }
        if let (Some((file, line, column)), Some(lint)) = (location, lint) {
            let diagnostic = ClippyDiagnostic {
                file: crate::compile::normalize_file(root, &file),
                line,
                column,
                lint,
                message: message.to_string(),
            };
            if is_error {
                return Some(diagnostic);
            }
            if first_warning.is_none() {
                first_warning = Some(diagnostic);
            }
        }
    }
    first_warning
}

fn lint_diagnostic_message(line: &str) -> Option<&str> {
    let line = line.trim_start();
    line.strip_prefix("error: ")
        .or_else(|| line.strip_prefix("warning: "))
}

fn diagnostic_location(line: &str) -> Option<(String, u32, u32)> {
    let line = line.trim_start();
    let location = line
        .strip_prefix("-->")
        .or_else(|| line.strip_prefix(":::"))?
        .trim();
    let mut parts = location.rsplitn(3, ':');
    let column = parts.next()?.parse().ok()?;
    let line_number = parts.next()?.parse().ok()?;
    let file = parts.next()?.trim();
    (!file.is_empty()).then(|| (file.to_string(), line_number, column))
}

fn clippy_lint_name(line: &str) -> Option<String> {
    let (name, is_clippy) = if let Some((_, name)) = line.split_once("index.html#") {
        (name, true)
    } else if let Some((_, name)) = line.split_once("clippy::") {
        (name, true)
    } else {
        let name = ["#[warn(", "#[deny(", "#[allow("]
            .into_iter()
            .find_map(|marker| line.split_once(marker).map(|(_, name)| name))?;
        (name, false)
    };
    let name: String = name
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect();
    if name.is_empty() {
        return None;
    }
    if is_clippy {
        Some(format!("clippy::{name}"))
    } else {
        Some(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clippy_workspace_flag_tracks_shell_commands_and_scope() {
        assert_eq!(
            clippy_workspace("cargo clippy", true),
            "cargo clippy --workspace"
        );
        assert_eq!(
            clippy_workspace("cargo clippy -- -D warnings", true),
            "cargo clippy --workspace -- -D warnings"
        );
        assert_eq!(
            clippy_workspace("cargo clippy --workspace", true),
            "cargo clippy --workspace"
        );
        assert_eq!(
            clippy_workspace("cargo fmt --check && cargo clippy -- -D warnings", true),
            "cargo fmt --check && cargo clippy --workspace -- -D warnings"
        );
        assert_eq!(
            clippy_workspace("cargo +stable clippy -- -D warnings", true),
            "cargo +stable clippy --workspace -- -D warnings"
        );
        assert_eq!(
            clippy_workspace("ENV=x cargo clippy -- -D warnings", true),
            "ENV=x cargo clippy --workspace -- -D warnings"
        );
        assert_eq!(
            clippy_workspace("if test -n \"$X\"; then cargo clippy; fi", true),
            "if test -n \"$X\"; then cargo clippy --workspace; fi"
        );
        assert_eq!(
            clippy_workspace("if cargo clippy --workspace; then echo ok; fi", false),
            "if cargo clippy; then echo ok; fi"
        );
        assert_eq!(
            clippy_workspace("{ cargo clippy; }", true),
            "{ cargo clippy --workspace; }"
        );
        assert_eq!(
            clippy_workspace("{ cargo clippy --workspace; }", false),
            "{ cargo clippy; }"
        );
        assert_eq!(
            clippy_workspace("cargo clippy --workspace -- -D warnings", false),
            "cargo clippy -- -D warnings"
        );
        assert_eq!(
            clippy_workspace("cargo clippy --workspace", false),
            "cargo clippy"
        );
        assert_eq!(clippy_workspace("cargo clippy", false), "cargo clippy");
        assert_eq!(clippy_workspace("npm test", true), "npm test");
    }

    fn token_values(script: &str) -> Vec<String> {
        shell_tokens(script)
            .iter()
            .filter(|token| !token.separator)
            .map(|token| token.value.clone())
            .collect()
    }

    #[test]
    fn shell_tokens_skips_comments_and_blank_separators() {
        assert!(shell_tokens("").is_empty());
        assert!(shell_tokens("   ").is_empty());
        assert!(shell_tokens("# only a comment").is_empty());
        assert_eq!(token_values("cargo # trailing comment"), ["cargo"]);
        assert_eq!(token_values("cargo\tclippy"), ["cargo", "clippy"]);
        let newline = shell_tokens("cargo\nclippy");
        assert_eq!(newline.len(), 3);
        assert!(newline[1].separator);
        assert_eq!(newline[0].value, "cargo");
        assert_eq!(newline[2].value, "clippy");
        let carriage = shell_tokens("cargo\rclippy");
        assert_eq!(carriage.len(), 3);
        assert!(carriage[1].separator);
    }

    #[test]
    fn shell_tokens_splits_operators_and_double_operators() {
        let separators = shell_tokens("a;b|c&d(e)f{g}h");
        assert!(separators.iter().any(|token| token.separator));
        assert_eq!(token_values("a&&b||c"), ["a", "b", "c"]);
        assert_eq!(
            shell_tokens("a&&b").iter().filter(|t| t.separator).count(),
            1
        );
    }

    #[test]
    fn shell_tokens_handles_quotes_and_escapes() {
        assert_eq!(token_values("echo 'a b'"), ["echo", "a b"]);
        assert_eq!(token_values("echo \"a b\""), ["echo", "a b"]);
        assert_eq!(token_values("echo \"a\\\"b\""), ["echo", "a\"b"]);
        assert_eq!(token_values("echo 'a\\b'"), ["echo", "a\\b"]);
        assert_eq!(token_values("echo a\\ b"), ["echo", "a b"]);
        assert_eq!(token_values("echo a\\\nb"), ["echo", "ab"]);
        assert_eq!(token_values("echo a\\"), ["echo", "a"]);
        assert_eq!(
            token_values("echo \"unterminated"),
            ["echo", "unterminated"]
        );
        assert_eq!(
            token_values("echo 'un\\terminated"),
            ["echo", "un\\terminated"]
        );
        assert_eq!(token_values("echo \"ab\\"), ["echo", "ab"]);
        assert_eq!(
            clippy_workspace("VAR='a b' cargo clippy", true),
            "VAR='a b' cargo clippy --workspace"
        );
    }
}

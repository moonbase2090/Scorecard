use std::path::Path;
use std::process::Command;

use crate::command::run_cmd_input;

use super::which;

/// `-I` and `-D` from global `CFLAGS`, `CPPFLAGS`, and `CXXFLAGS`.
/// `make` expands a reference such as `$(DEFS)` and leaves out an `ifeq`
/// branch it does not take. The types gate stays advisory unless every
/// recipe uses only the known compiler variables and the Makefile does not
/// include a file, run a shell, call `$(eval)` or `$(file)`, or build a
/// subdirectory. `make` is not run in those rewrite cases.
pub(crate) struct MakefileFlags {
    pub(crate) c: Vec<String>,
    pub(crate) cxx: Vec<String>,
    pub(crate) uncertain: bool,
}

impl MakefileFlags {
    pub(crate) fn none() -> Self {
        Self {
            c: Vec::new(),
            cxx: Vec::new(),
            uncertain: false,
        }
    }

    pub(crate) fn read_with_generated(
        root: &Path,
        exclude: &[String],
        include_generated: &[String],
    ) -> Self {
        let Some((name, text)) = makefile_source(root) else {
            return Self::none();
        };
        let mut shape = makefile_shape(&text);
        if nested_makefile(root, exclude, include_generated) {
            shape.partial = true;
        }
        if !shape.skip_make {
            if let Some((cpp, c, cxx)) = flags_from_make(root, name) {
                let (c, cxx) = combine_flag_text(&cpp, &c, &cxx);
                return Self {
                    c,
                    cxx,
                    uncertain: shape.partial,
                };
            }
        }
        let (c, cxx) = flags_from_text(&text);
        Self {
            c,
            cxx,
            uncertain: shape.skip_make || shape.partial || makefile_text_uncertain(&text),
        }
    }

    #[cfg(test)]
    pub(crate) fn read(root: &Path) -> Self {
        Self::read_with_generated(root, &[], &[])
    }
}

struct MakefileShape {
    /// An include or a shell command runs when `make` reads the file.
    skip_make: bool,
    /// A recipe or target-specific flag is invisible to `$(CFLAGS)`.
    partial: bool,
}

fn makefile_shape(text: &str) -> MakefileShape {
    let mut skip_make = false;
    let mut partial = false;
    for line in text.lines() {
        let code = strip_makefile_comment(line);
        if calls_make_function(code, "shell")
            || calls_make_function(code, "eval")
            || calls_make_function(code, "file")
        {
            skip_make = true;
        }
        if line.starts_with('\t') {
            if recipe_is_partial(code) {
                partial = true;
            }
            continue;
        }
        if let Some(recipe) = recipe_after_semicolon(code.trim()) {
            if recipe_is_partial(recipe) {
                partial = true;
            }
        }
        if is_include_directive(code.trim()) {
            skip_make = true;
        }
        if let Some(parsed) = make_lhs(line) {
            if parsed.shell {
                skip_make = true;
            }
            if parsed.prefix.contains(':')
                && matches!(parsed.name, "CFLAGS" | "CXXFLAGS" | "CPPFLAGS")
            {
                partial = true;
            }
        }
    }
    MakefileShape { skip_make, partial }
}

fn calls_make_function(code: &str, name: &str) -> bool {
    let bytes = code.as_bytes();
    let mut index = 0;
    while index + 2 < bytes.len() {
        if bytes[index] == b'$' && (bytes[index + 1] == b'(' || bytes[index + 1] == b'{') {
            let mut start = index + 2;
            while start < bytes.len() && bytes[start].is_ascii_whitespace() {
                start += 1;
            }
            if code[start..].starts_with(name) {
                let end = start + name.len();
                let boundary = code[end..].chars().next();
                if boundary.is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_') {
                    return true;
                }
            }
        }
        index += 1;
    }
    false
}

fn recipe_is_partial(line: &str) -> bool {
    recipe_compile_flag(line) || recipe_recurses(line) || recipe_uses_unknown_variable(line)
}

fn recipe_after_semicolon(line: &str) -> Option<&str> {
    let semi = line.find(';')?;
    let before = &line[..semi];
    if before.contains(':') {
        return Some(line[semi + 1..].trim());
    }
    None
}

fn recipe_recurses(line: &str) -> bool {
    line.split_whitespace().any(|token| {
        let token = token.trim_matches(['"', '\'']);
        token == "make"
            || token == "-C"
            || token.starts_with("-C/")
            || token.starts_with("-C.")
            || (token.starts_with("-C")
                && token.len() > 2
                && token[2..]
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_ascii_lowercase()))
    })
}

fn recipe_uses_unknown_variable(line: &str) -> bool {
    let chars: Vec<char> = line.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] != '$' {
            index += 1;
            continue;
        }
        let Some(next) = chars.get(index + 1).copied() else {
            return true;
        };
        if next == '$' || matches!(next, '@' | '<' | '^' | '*' | '?') {
            index += 2;
            continue;
        }
        if next != '(' && next != '{' {
            return true;
        }
        let close = if next == '(' { ')' } else { '}' };
        let mut depth = 1;
        let mut end = index + 2;
        let start = end;
        while end < chars.len() {
            if chars[end] == next {
                depth += 1;
            } else if chars[end] == close {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            end += 1;
        }
        if end >= chars.len() {
            return true;
        }
        let inner: String = chars[start..end].iter().collect();
        let inner = inner.trim();
        if inner.contains(':') || inner.contains('$') || !known_make_variable(inner) {
            return true;
        }
        index = end + 1;
    }
    false
}

fn known_make_variable(name: &str) -> bool {
    matches!(
        name,
        "CC" | "CXX"
            | "CFLAGS"
            | "CPPFLAGS"
            | "CXXFLAGS"
            | "LDFLAGS"
            | "LDLIBS"
            | "TARGET_ARCH"
            | "@"
            | "<"
            | "^"
            | "*"
            | "?"
    )
}

fn nested_makefile(root: &Path, exclude: &[String], include_generated: &[String]) -> bool {
    sc_graph::walk_files(root, root, exclude, include_generated, Some(6))
        .into_iter()
        .any(|path| {
            path.parent() != Some(root)
                && matches!(
                    path.file_name().and_then(|name| name.to_str()),
                    Some("Makefile" | "makefile" | "GNUmakefile")
                )
        })
}

fn is_include_directive(line: &str) -> bool {
    ["-include", "sinclude", "include"]
        .into_iter()
        .any(|prefix| {
            line.strip_prefix(prefix)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
        })
}

fn recipe_compile_flag(line: &str) -> bool {
    line.split_whitespace().any(|token| {
        let token = token.trim_matches(['"', '\'']);
        token.starts_with("-D")
            || token.starts_with("-I")
            || token.starts_with("-isystem")
            || token.starts_with("-include")
    })
}

fn makefile_source(root: &Path) -> Option<(&str, String)> {
    ["Makefile", "makefile", "GNUmakefile"]
        .into_iter()
        .find_map(|name| {
            std::fs::read_to_string(root.join(name))
                .ok()
                .map(|text| (name, text.replace("\\\n", " ")))
        })
}

fn flags_from_make(root: &Path, makefile_name: &str) -> Option<(String, String, String)> {
    if !which("make") {
        return None;
    }
    // `$(info)` prints the expanded value. GNU make 3.81 has no `--eval`,
    // so the probe is a makefile on stdin. `-o` treats the project's
    // makefile as up to date. The goal is not the default target, and its
    // recipe does not build anything.
    let body = "\
$(info __SC_CPPFLAGS__=$(CPPFLAGS))\n\
$(info __SC_CFLAGS__=$(CFLAGS))\n\
$(info __SC_CXXFLAGS__=$(CXXFLAGS))\n\
__sc_flags:\n\
\t@:\n";
    let mut cmd = Command::new("make");
    cmd.current_dir(root)
        .args(["-s", "--no-print-directory", "-o"])
        .arg(makefile_name)
        .arg("-f")
        .arg(makefile_name)
        .args(["-f", "-", "__sc_flags"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let captured = run_cmd_input(
        &mut cmd,
        std::time::Duration::from_secs(5),
        Some(body.as_bytes()),
    )
    .ok()?;
    if !captured.status.success() {
        return None;
    }
    Some((
        marked_make_value(&captured.stdout, "CPPFLAGS")?,
        marked_make_value(&captured.stdout, "CFLAGS")?,
        marked_make_value(&captured.stdout, "CXXFLAGS")?,
    ))
}

fn marked_make_value(stdout: &str, name: &str) -> Option<String> {
    let prefix = format!("__SC_{name}__=");
    stdout
        .lines()
        .rev()
        .find(|line| line.starts_with(&prefix))
        .map(|line| line[prefix.len()..].to_string())
}

fn combine_flag_text(cpp: &str, c: &str, cxx: &str) -> (Vec<String>, Vec<String>) {
    // `make`'s `COMPILE.c` is `$(CC) $(CFLAGS) $(CPPFLAGS)`, and `COMPILE.cc`
    // is `$(CXX) $(CXXFLAGS) $(CPPFLAGS)`. The last flag wins.
    let cpp_tokens = compile_flag_tokens(cpp);
    let mut c_flags = compile_flag_tokens(c);
    c_flags.extend(cpp_tokens.iter().cloned());
    let mut cxx_flags = compile_flag_tokens(cxx);
    cxx_flags.extend(cpp_tokens);
    (c_flags, cxx_flags)
}

pub(crate) fn makefile_text_uncertain(text: &str) -> bool {
    text.contains("$(")
        || text.contains("${")
        || text.lines().any(|line| {
            let line = strip_makefile_comment(line).trim();
            line.starts_with("ifeq")
                || line.starts_with("ifneq")
                || line.starts_with("ifdef")
                || line.starts_with("ifndef")
                || line == "else"
                || line.starts_with("else ")
                || line == "endif"
                || line.starts_with("endif ")
        })
}

fn flags_from_text(text: &str) -> (Vec<String>, Vec<String>) {
    let mut cflags = MakeValue::default();
    let mut cxxflags = MakeValue::default();
    let mut cppflags = MakeValue::default();
    for line in text.lines() {
        let Some((name, op, value)) = makefile_assignment(line) else {
            continue;
        };
        let slot = match name {
            "CFLAGS" => &mut cflags,
            "CXXFLAGS" => &mut cxxflags,
            "CPPFLAGS" => &mut cppflags,
            _ => continue,
        };
        apply_make_value(slot, op, value);
    }
    combine_flag_text(&cppflags.text, &cflags.text, &cxxflags.text)
}

#[derive(Default)]
struct MakeValue {
    text: String,
    set: bool,
}

enum MakeAssign {
    Set,
    Append,
    IfUnset,
}

fn apply_make_value(slot: &mut MakeValue, op: MakeAssign, value: &str) {
    match op {
        MakeAssign::IfUnset if slot.set => {}
        MakeAssign::Append => {
            if slot.set && !slot.text.is_empty() && !value.is_empty() {
                slot.text.push(' ');
            }
            slot.text.push_str(value);
            slot.set = true;
        }
        MakeAssign::Set | MakeAssign::IfUnset => {
            slot.text = value.to_string();
            slot.set = true;
        }
    }
}

struct MakeLhs<'a> {
    prefix: &'a str,
    name: &'a str,
    op: MakeAssign,
    value: &'a str,
    /// `!=` runs a shell when `make` reads the file.
    shell: bool,
}

fn make_lhs(line: &str) -> Option<MakeLhs<'_>> {
    let line = strip_makefile_comment(line).trim();
    let eq = line.find('=')?;
    let before = &line[..eq];
    let shell = before.ends_with('!') && !before.ends_with("::");
    let (lhs, op) = if let Some(lhs) = before.strip_suffix("::") {
        (lhs, MakeAssign::Set)
    } else if let Some(lhs) = before.strip_suffix(':') {
        (lhs, MakeAssign::Set)
    } else if let Some(lhs) = before.strip_suffix('?') {
        (lhs, MakeAssign::IfUnset)
    } else if let Some(lhs) = before.strip_suffix('+') {
        (lhs, MakeAssign::Append)
    } else if let Some(lhs) = before.strip_suffix('!') {
        (lhs, MakeAssign::Set)
    } else {
        (before, MakeAssign::Set)
    };
    let lhs = lhs.trim_end();
    let (prefix, name) = match lhs.rsplit_once(char::is_whitespace) {
        Some((prefix, name)) => (prefix, name),
        None => ("", lhs),
    };
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return None;
    }
    Some(MakeLhs {
        prefix,
        name,
        op,
        value: line[eq + 1..].trim(),
        shell,
    })
}

fn makefile_assignment(line: &str) -> Option<(&str, MakeAssign, &str)> {
    let parsed = make_lhs(line)?;
    // `demo: CFLAGS +=` is not a global flag. Applying it would hide a
    // compile error that the real build does not have.
    if parsed.shell || parsed.prefix.contains(':') {
        return None;
    }
    Some((parsed.name, parsed.op, parsed.value))
}

fn strip_makefile_comment(line: &str) -> &str {
    let mut quote = None;
    for (index, ch) in line.char_indices() {
        match ch {
            '"' | '\'' if quote.is_none() => quote = Some(ch),
            ch if Some(ch) == quote => quote = None,
            '#' if quote.is_none() => return &line[..index],
            _ => {}
        }
    }
    line
}

fn compile_flag_tokens(value: &str) -> Vec<String> {
    let chars: Vec<char> = value.chars().collect();
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        // `\"` is the quote `make` passes through, and `\ ` is a space that
        // stays inside the same compiler argument.
        if ch == '\\' && matches!(chars.get(index + 1), Some('"' | '\'' | ' ')) {
            current.push(chars[index + 1]);
            index += 2;
            continue;
        }
        match ch {
            '"' | '\'' if quote.is_none() => quote = Some(ch),
            ch if Some(ch) == quote => quote = None,
            ch if ch.is_whitespace() && quote.is_none() => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
        index += 1;
    }
    if !current.is_empty() {
        out.push(current);
    }
    out.into_iter()
        .filter(|token| {
            token.starts_with("-I")
                || token.starts_with("-D")
                || token.starts_with("-U")
                || token.starts_with("-isystem")
                || token.starts_with("-include")
        })
        .collect()
}

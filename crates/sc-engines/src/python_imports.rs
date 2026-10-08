use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub(super) fn is_local(root: &Path, file: &Path, name: &str) -> bool {
    if name == "conftest" && conftest_above(root, file) {
        return true;
    }
    source_roots(root, file)
        .iter()
        .any(|dir| module_on(dir, name))
}

fn conftest_above(root: &Path, file: &Path) -> bool {
    let mut dir = file.parent();
    while let Some(current) = dir {
        if !current.starts_with(root) {
            break;
        }
        if current.join("conftest.py").is_file() {
            return true;
        }
        if current == root {
            break;
        }
        dir = current.parent();
    }
    false
}

fn source_roots(root: &Path, file: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    roots.push(root.to_path_buf());
    for name in ["src", "tests", "test"] {
        let dir = root.join(name);
        if dir.is_dir() {
            roots.push(dir);
        }
    }
    roots.extend(pytest_pythonpath(root));
    if let Some(parent) = file.parent() {
        if parent.starts_with(root) && import_path_dir(root, parent) {
            roots.push(parent.to_path_buf());
        }
    }
    roots
}

fn import_path_dir(root: &Path, dir: &Path) -> bool {
    if dir.join("__init__.py").is_file() {
        return false;
    }
    let Ok(rel) = dir.strip_prefix(root) else {
        return false;
    };
    matches!(
        rel.components().next().and_then(|c| c.as_os_str().to_str()),
        Some("tests" | "test")
    )
}

fn module_on(dir: &Path, name: &str) -> bool {
    if name.is_empty() || name.contains(['/', '\\']) {
        return false;
    }
    if dir.join(format!("{name}.py")).is_file() {
        return true;
    }
    let pkg = dir.join(name);
    if !pkg.is_dir() {
        return false;
    }
    if pkg.join("__init__.py").is_file() {
        return true;
    }
    let Ok(entries) = std::fs::read_dir(&pkg) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        path.extension().and_then(|ext| ext.to_str()) == Some("py")
            || (path.is_dir() && path.join("__init__.py").is_file())
    })
}

fn pytest_pythonpath(root: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(text) = std::fs::read_to_string(root.join("pyproject.toml")) {
        paths.extend(pythonpath_from_pyproject(&text, root));
    }
    for (file, header) in [
        ("pytest.ini", "[pytest]"),
        ("tox.ini", "[pytest]"),
        ("setup.cfg", "[tool:pytest]"),
    ] {
        if let Ok(text) = std::fs::read_to_string(root.join(file)) {
            paths.extend(pythonpath_from_ini(&text, header, root));
        }
    }
    paths
}

fn pythonpath_from_pyproject(text: &str, root: &Path) -> Vec<PathBuf> {
    let mut in_pytest = false;
    let mut in_array = false;
    let mut paths = Vec::new();
    for line in text.lines() {
        let trimmed = strip_comment(line).trim();
        if trimmed.starts_with('[') {
            in_pytest = trimmed == "[tool.pytest.ini_options]";
            in_array = false;
            continue;
        }
        if !in_pytest {
            continue;
        }
        if !in_array && !trimmed.starts_with("pythonpath") {
            continue;
        }
        if !in_array && trimmed.starts_with("pythonpath") && trimmed.contains('[') {
            in_array = true;
        } else if !in_array {
            if let Some(value) = table_string(trimmed, "pythonpath") {
                push_rel(&mut paths, root, &value);
            }
            continue;
        }
        push_quoted(&mut paths, root, trimmed);
        if trimmed.contains(']') {
            in_array = false;
        }
    }
    paths
}

fn pythonpath_from_ini(text: &str, header: &str, root: &Path) -> Vec<PathBuf> {
    let mut in_section = false;
    let mut collecting = false;
    let mut paths = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_section = trimmed.eq_ignore_ascii_case(header);
            collecting = false;
            continue;
        }
        if !in_section || trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';')
        {
            if trimmed.is_empty() {
                collecting = false;
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("pythonpath") {
            let rest = rest.trim();
            if let Some(value) = rest.strip_prefix('=') {
                let value = value.trim();
                if value.is_empty() {
                    collecting = true;
                } else {
                    push_words(&mut paths, root, value);
                    collecting = false;
                }
            }
            continue;
        }
        if collecting && (line.starts_with(' ') || line.starts_with('\t')) {
            push_words(&mut paths, root, trimmed);
        } else {
            collecting = false;
        }
    }
    paths
}

fn push_quoted(paths: &mut Vec<PathBuf>, root: &Path, line: &str) {
    for token in quoted_tokens(line) {
        push_rel(paths, root, &token);
    }
}

fn push_words(paths: &mut Vec<PathBuf>, root: &Path, line: &str) {
    for part in line.split_whitespace() {
        push_rel(paths, root, part);
    }
}

fn push_rel(paths: &mut Vec<PathBuf>, root: &Path, raw: &str) {
    let raw = raw.trim().trim_matches(['"', '\'']);
    if raw.is_empty() {
        return;
    }
    let path = root.join(raw);
    if path.is_dir() {
        paths.push(path);
    }
}

pub(super) fn installed_modules(root: &Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for site in site_packages(root) {
        names.append(&mut site_module_names(&site));
    }
    names
}

fn site_module_names(site: &Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let Ok(entries) = std::fs::read_dir(site) else {
        return names;
    };
    for entry in entries.flatten() {
        names.append(&mut entry_module_names(&entry.path()));
    }
    names
}

fn entry_module_names(path: &Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let Some(fname) = path.file_name().and_then(|s| s.to_str()) else {
        return names;
    };
    if fname.ends_with(".dist-info") {
        if let Ok(text) = std::fs::read_to_string(path.join("top_level.txt")) {
            names.extend(top_level_names(&text));
        }
        return names;
    }
    if let Some(stem) = fname.strip_suffix(".py") {
        if !stem.is_empty() {
            names.insert(normalize_mod(stem));
        }
        return names;
    }
    if path.is_dir() && path.join("__init__.py").is_file() {
        names.insert(normalize_mod(fname));
    }
    names
}

fn top_level_names(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for line in text.lines() {
        let name = normalize_mod(line.trim());
        if !name.is_empty() {
            names.insert(name);
        }
    }
    names
}

fn site_packages(root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    push_sites(&root.join(".venv"), &mut dirs);
    push_sites(&root.join("venv"), &mut dirs);
    if let Some(venv) = std::env::var_os("VIRTUAL_ENV") {
        let path = PathBuf::from(venv);
        if path.starts_with(root) {
            push_sites(&path, &mut dirs);
        }
    }
    dirs
}

fn push_sites(venv: &Path, out: &mut Vec<PathBuf>) {
    let lib = venv.join("lib");
    if let Ok(entries) = std::fs::read_dir(&lib) {
        for entry in entries.flatten() {
            let site = entry.path().join("site-packages");
            if site.is_dir() {
                out.push(site);
            }
        }
    }
    let windows = venv.join("Lib").join("site-packages");
    if windows.is_dir() {
        out.push(windows);
    }
}

pub(super) fn local_packages(root: &Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for base in [root.join("src"), root.to_path_buf()] {
        let Ok(entries) = std::fs::read_dir(&base) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join("__init__.py").is_file() {
                if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                    names.insert(normalize_mod(name));
                }
            }
        }
    }
    names
}

pub(super) fn python_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for dir in [root.join("src"), root.join("tests")] {
        collect_py(&dir, 0, &mut out);
    }
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("py") {
                out.push(path);
            }
        }
    }
    out
}

fn collect_py(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    if depth > 6 || out.len() >= 400 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name.starts_with('.') || name == "__pycache__" || name == ".venv" {
            continue;
        }
        if path.is_dir() {
            collect_py(&path, depth + 1, out);
        } else if name.ends_with(".py") {
            out.push(path);
        }
    }
}

pub fn import_roots(text: &str) -> Vec<(String, u32)> {
    let masked = code_only(text);
    let optional = optional_import_lines(&masked);
    let mut out = Vec::new();
    for (index, line) in masked.lines().enumerate() {
        let line_no = index as u32 + 1;
        if optional.contains(&line_no) {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.starts_with('#')
            || trimmed.starts_with("from .")
            || trimmed.starts_with("from __future__")
        {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("import ") {
            for part in rest.split(',') {
                let module = part
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .split('.')
                    .next()
                    .unwrap_or("");
                if module_name(module) {
                    out.push((module.to_string(), line_no));
                }
            }
        } else if let Some(rest) = trimmed.strip_prefix("from ") {
            let module = rest
                .split_whitespace()
                .next()
                .unwrap_or("")
                .split('.')
                .next()
                .unwrap_or("");
            if module_name(module) && module != "__future__" {
                out.push((module.to_string(), line_no));
            }
        }
    }
    out
}

fn module_name(name: &str) -> bool {
    !name.is_empty() && name != "*" && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Replace comments and string contents with spaces, keeping newlines, so a
/// docstring cannot look like an import.
fn code_only(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if let Some((quote, triple, raw, skip)) = string_at(&chars[i..]) {
            for _ in 0..skip {
                out.push(if chars[i] == '\n' { '\n' } else { ' ' });
                i += 1;
            }
            if triple {
                let closer = [quote, quote, quote];
                while i < chars.len() {
                    if chars[i..].starts_with(&closer) {
                        out.push_str("   ");
                        i += 3;
                        break;
                    }
                    let c = chars[i];
                    out.push(if c == '\n' { '\n' } else { ' ' });
                    i += 1;
                }
            } else {
                while i < chars.len() {
                    let c = chars[i];
                    if !raw && c == '\\' {
                        out.push(' ');
                        i += 1;
                        if i < chars.len() {
                            out.push(if chars[i] == '\n' { '\n' } else { ' ' });
                            i += 1;
                        }
                        continue;
                    }
                    out.push(if c == '\n' { '\n' } else { ' ' });
                    i += 1;
                    if c == quote || c == '\n' {
                        break;
                    }
                }
            }
            continue;
        }
        if chars[i] == '#' {
            while i < chars.len() && chars[i] != '\n' {
                out.push(' ');
                i += 1;
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn string_at(rest: &[char]) -> Option<(char, bool, bool, usize)> {
    let mut i = 0;
    let mut raw = false;
    while i < rest.len()
        && i < 3
        && matches!(rest[i], 'r' | 'R' | 'b' | 'B' | 'f' | 'F' | 'u' | 'U')
    {
        if matches!(rest[i], 'r' | 'R') {
            raw = true;
        }
        i += 1;
    }
    let quote = *rest.get(i)?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let triple = rest.get(i + 1) == Some(&quote) && rest.get(i + 2) == Some(&quote);
    Some((quote, triple, raw, i + if triple { 3 } else { 1 }))
}

fn optional_import_lines(text: &str) -> BTreeSet<u32> {
    let mut skip = BTreeSet::new();
    let mut stack: Vec<(usize, bool, Vec<u32>)> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let line_no = index as u32 + 1;
        while let Some((try_indent, caught, lines)) = stack.last() {
            let clause = trimmed.starts_with("except")
                || trimmed.starts_with("else:")
                || trimmed.starts_with("finally:");
            if indent < *try_indent || (indent == *try_indent && !clause) {
                let (_, caught, lines) = stack.pop().unwrap();
                if caught {
                    skip.extend(lines);
                }
            } else {
                let _ = (caught, lines);
                break;
            }
        }
        if trimmed == "try:" || trimmed.starts_with("try:") {
            stack.push((indent, false, Vec::new()));
            continue;
        }
        if let Some((try_indent, caught, lines)) = stack.last_mut() {
            if indent == *try_indent
                && trimmed.starts_with("except")
                && (trimmed.contains("ImportError") || trimmed.contains("ModuleNotFoundError"))
            {
                *caught = true;
            }
            if indent > *try_indent
                && (trimmed.starts_with("import ") || trimmed.starts_with("from "))
            {
                lines.push(line_no);
            }
        }
    }
    for (_, caught, lines) in stack {
        if caught {
            skip.extend(lines);
        }
    }
    skip
}

pub(super) fn declaration_place(root: &Path) -> String {
    if root.join("pyproject.toml").is_file() {
        "pyproject.toml".into()
    } else if root.join("requirements.txt").is_file() {
        "requirements.txt".into()
    } else if root.join("setup.cfg").is_file() {
        "setup.cfg".into()
    } else if root.join("setup.py").is_file() {
        "setup.py".into()
    } else {
        "pyproject.toml".into()
    }
}

pub(super) fn declared_elsewhere(root: &Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    if let Ok(text) = std::fs::read_to_string(root.join("requirements.txt")) {
        names.extend(requirements_modules(&text));
    }
    if let Ok(text) = std::fs::read_to_string(root.join("setup.cfg")) {
        names.extend(setup_cfg_requires(&text));
    }
    if let Ok(text) = std::fs::read_to_string(root.join("setup.py")) {
        names.extend(setup_py_requires(&text));
    }
    names
}

fn requirements_modules(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for line in text.lines() {
        if let Some(name) = requirement_name(line) {
            names.insert(name);
        }
    }
    names
}

fn requirement_name(line: &str) -> Option<String> {
    let line = strip_comment(line).trim();
    if line.is_empty() || line.starts_with('-') {
        return None;
    }
    let name = line
        .split(['>', '<', '=', '!', '~', '[', ';', ' ', '\\'])
        .next()
        .unwrap_or("");
    if name.is_empty() {
        None
    } else {
        Some(normalize_mod(name))
    }
}

fn setup_cfg_requires(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut in_options = false;
    let mut in_requires = false;
    for line in text.lines() {
        let trimmed = strip_comment(line).trim();
        if trimmed.starts_with('[') {
            in_options = trimmed == "[options]";
            in_requires = false;
            continue;
        }
        if !in_options {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("install_requires") {
            let rest = rest.trim().trim_start_matches('=').trim();
            if let Some(name) = requirement_name(rest) {
                names.insert(name);
            }
            in_requires = true;
            continue;
        }
        if in_requires {
            if !line.starts_with(char::is_whitespace) && !trimmed.is_empty() {
                in_requires = false;
                continue;
            }
            if let Some(name) = requirement_name(trimmed) {
                names.insert(name);
            }
        }
    }
    names
}

fn setup_py_requires(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut rest = text;
    while let Some(index) = rest.find("install_requires") {
        let line_start = rest[..index].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let commented = rest[line_start..index].trim_start().starts_with('#');
        rest = &rest[index + "install_requires".len()..];
        if commented {
            continue;
        }
        let after = rest.trim_start().trim_start_matches('=').trim_start();
        if after.starts_with('[') || after.starts_with('(') {
            let mut depth = 0;
            let mut chunk = String::new();
            for c in after.chars() {
                chunk.push(c);
                if c == '[' || c == '(' {
                    depth += 1;
                }
                if c == ']' || c == ')' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
            collect_quoted(&chunk, &mut names);
        }
    }
    names
}

pub(super) fn project_name(text: &str) -> Option<String> {
    let mut in_project = false;
    for line in text.lines() {
        let trimmed = strip_comment(line).trim();
        if trimmed.starts_with('[') {
            in_project = trimmed == "[project]";
            continue;
        }
        if in_project {
            if let Some(name) = table_string(trimmed, "name") {
                return Some(name);
            }
        }
    }
    None
}

pub(super) fn dependency_modules(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut in_project = false;
    let mut in_array = false;
    let mut in_extra = false;
    for line in text.lines() {
        let trimmed = strip_comment(line).trim();
        if trimmed.starts_with('[') {
            in_project = trimmed == "[project]";
            in_extra = trimmed.contains("optional-dependencies")
                || trimmed.starts_with("[dependency-groups");
            in_array = false;
            continue;
        }
        if in_project && (trimmed.starts_with("dependencies") || in_array) {
            if trimmed.starts_with("dependencies") && trimmed.contains('[') && trimmed.contains(']')
            {
                collect_quoted(trimmed, &mut names);
                continue;
            }
            if trimmed.starts_with("dependencies") && trimmed.contains('[') {
                in_array = true;
            }
            if in_array {
                collect_quoted(trimmed, &mut names);
                if trimmed.contains(']') {
                    in_array = false;
                }
            }
            continue;
        }
        if in_extra {
            collect_quoted(trimmed, &mut names);
        }
    }
    names
}

fn quoted_tokens(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find('"').or_else(|| rest.find('\'')) {
        let quote = rest.as_bytes()[start] as char;
        rest = &rest[start + 1..];
        let Some(end) = rest.find(quote) else {
            break;
        };
        out.push(rest[..end].to_string());
        rest = &rest[end + 1..];
    }
    out
}

fn collect_quoted(line: &str, names: &mut BTreeSet<String>) {
    for token in quoted_tokens(line) {
        let name = token
            .split(['>', '<', '=', '!', '~', '[', ';', ' '])
            .next()
            .unwrap_or("");
        if !name.is_empty() {
            names.insert(normalize_mod(name));
        }
    }
}

fn table_string(line: &str, key: &str) -> Option<String> {
    let (left, right) = line.split_once('=')?;
    if left.trim() != key {
        return None;
    }
    let value = right.trim().trim_matches('"').trim_matches('\'');
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn strip_comment(line: &str) -> &str {
    match line.find('#') {
        Some(index) => &line[..index],
        None => line,
    }
}

pub(super) fn normalize_mod(name: &str) -> String {
    name.trim().replace('-', "_").to_ascii_lowercase()
}

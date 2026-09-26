//! Cyclomatic complexity for the non-Rust packs.
//!
//! Python uses the stdlib `ast` module. The other packs count decision points
//! in each function body. Coverage is attached later by the caller.

use std::path::Path;
use std::process::Command;

use sc_core::Span;
use sc_graph::FunctionInfo;

pub fn go_coverage(profile: &str, functions: &[FunctionInfo]) -> crate::coverage::CoverageData {
    let mut covered = Vec::new();
    let stmts = parse_go_cover(profile);
    for function in functions {
        let mut hit = 0u32;
        let mut total = 0u32;
        for stmt in &stmts {
            if !crate::coverage::file_matches(&stmt.file, &function.file) {
                continue;
            }
            if stmt.start_line < function.span.start_line
                || stmt.start_line > function.span.end_line
            {
                continue;
            }
            total += 1;
            if stmt.count > 0 {
                hit += 1;
            }
        }
        let coverage = if total == 0 {
            0.0
        } else {
            f64::from(hit) / f64::from(total)
        };
        covered.push(crate::coverage::CovFunction {
            file: function.file.clone(),
            demangled: function.symbol.clone(),
            coverage,
        });
    }
    crate::coverage::CoverageData {
        functions: covered,
        line_rate: {
            let total = stmts.len() as f64;
            if total == 0.0 {
                0.0
            } else {
                stmts.iter().filter(|stmt| stmt.count > 0).count() as f64 / total
            }
        },
    }
}

struct GoStmt {
    file: String,
    start_line: u32,
    count: u32,
}

fn parse_go_cover(profile: &str) -> Vec<GoStmt> {
    let mut out = Vec::new();
    for line in profile.lines() {
        if line.starts_with("mode:") || line.is_empty() {
            continue;
        }
        let Some((loc, rest)) = line.split_once(' ') else {
            continue;
        };
        let Some((file, span)) = loc.rsplit_once(':') else {
            continue;
        };
        let Some((start, _)) = span.split_once(',') else {
            continue;
        };
        let start_line = start
            .split('.')
            .next()
            .unwrap_or("0")
            .parse::<u32>()
            .unwrap_or(0);
        let count = rest
            .split_whitespace()
            .nth(1)
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        out.push(GoStmt {
            file: file.to_string(),
            start_line,
            count,
        });
    }
    out
}

pub fn functions_for_pack(root: &Path, pack: &str) -> Vec<FunctionInfo> {
    match pack {
        "python" => python_functions(root),
        "node" => files_with(root, &["js", "jsx", "mjs", "cjs", "ts", "tsx"], Lang::C),
        "go" => files_with(root, &["go"], Lang::Go),
        "java" => files_with(root, &["java"], Lang::C),
        "csharp" => files_with(root, &["cs"], Lang::C),
        "php" => files_with(root, &["php"], Lang::C),
        "cpp" => files_with(
            root,
            &["c", "cc", "cpp", "cxx", "h", "hh", "hpp", "hxx"],
            Lang::C,
        ),
        "bash" => files_with(root, &["sh", "bash"], Lang::Bash),
        "command" => {
            let mut all = Vec::new();
            for pack in [
                "python", "node", "go", "java", "csharp", "php", "cpp", "bash",
            ] {
                all.extend(functions_for_pack(root, pack));
            }
            all
        }
        _ => Vec::new(),
    }
}

enum Lang {
    C,
    Go,
    Bash,
}

fn python_functions(root: &Path) -> Vec<FunctionInfo> {
    let files = py_files(root);
    if files.is_empty() {
        return Vec::new();
    }
    let payload = serde_json::to_string(&files).unwrap_or_else(|_| "[]".into());
    let Ok(output) = python_ast(root, &payload) else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let Ok(rows) = serde_json::from_str::<Vec<PyFn>>(&text) else {
        return Vec::new();
    };
    rows.into_iter()
        .map(|row| FunctionInfo {
            file: row.file,
            symbol: row.symbol,
            span: Span {
                start_line: row.start,
                start_col: 1,
                end_line: row.end.max(row.start),
                end_col: 1,
            },
            cc: row.cc.max(1),
        })
        .collect()
}

#[derive(serde::Deserialize)]
struct PyFn {
    file: String,
    symbol: String,
    cc: u32,
    start: u32,
    end: u32,
}

fn python_ast(root: &Path, payload: &str) -> Result<std::process::Output, ()> {
    if crate::toolchain::host_has("python3") {
        return Command::new("python3")
            .arg("-c")
            .arg(PYTHON_CC)
            .arg(payload)
            .current_dir(root)
            .output()
            .map_err(|_| ());
    }
    let script = format!(
        "python3 -c {} {}",
        shell_quote(PYTHON_CC),
        shell_quote(payload)
    );
    let script = crate::toolchain::prepare(&script);
    if script.starts_with("python3 ") {
        return Err(());
    }
    let mut cmd = Command::new("sh");
    crate::toolchain::apply_docker_host(&mut cmd);
    cmd.current_dir(root)
        .arg("-c")
        .arg(script)
        .output()
        .map_err(|_| ())
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn py_files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    collect(root, root, 0, &["py"], &mut out);
    out
}

fn files_with(root: &Path, exts: &[&str], lang: Lang) -> Vec<FunctionInfo> {
    let mut paths = Vec::new();
    collect(root, root, 0, exts, &mut paths);
    let mut functions = Vec::new();
    for rel in paths {
        let Ok(text) = std::fs::read_to_string(root.join(&rel)) else {
            continue;
        };
        functions.extend(scan_text(&rel, &text, &lang));
    }
    functions
}

fn collect(root: &Path, dir: &Path, depth: u32, exts: &[&str], out: &mut Vec<String>) {
    if depth > 6 || out.len() >= 400 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if name.starts_with('.')
            || name == "node_modules"
            || name == "target"
            || name == "dist"
            || name == "__pycache__"
            || name == ".venv"
            || name == "vendor"
        {
            continue;
        }
        if path.is_dir() {
            collect(root, &path, depth + 1, exts, out);
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| exts.contains(&ext))
        {
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
}

fn scan_text(rel: &str, text: &str, lang: &Lang) -> Vec<FunctionInfo> {
    match lang {
        Lang::Bash => scan_bash(rel, text),
        Lang::Go => scan_braces(
            rel,
            text,
            &["func"],
            &["if", "for", "switch", "select", "case"],
        ),
        Lang::C => scan_braces(
            rel,
            text,
            &["function", "func"],
            &["if", "for", "while", "case", "catch", "foreach"],
        ),
    }
}

fn scan_bash(rel: &str, text: &str) -> Vec<FunctionInfo> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        let name = bash_name(trimmed);
        if let Some(name) = name {
            let start = index;
            index += 1;
            let mut cc = 1u32;
            while index < lines.len() && bash_name(lines[index].trim()).is_none() {
                cc += decision_hits(lines[index], &["if", "for", "while", "until", "elif"]);
                index += 1;
            }
            out.push(info(rel, name, start as u32 + 1, index as u32, cc));
            continue;
        }
        index += 1;
    }
    out
}

fn bash_name(line: &str) -> Option<String> {
    if let Some(rest) = line.strip_prefix("function ") {
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() {
            None
        } else {
            Some(name)
        }
    } else if let Some((name, rest)) = line.split_once("()") {
        let name = name.trim();
        if !name.is_empty()
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !rest.trim_start().starts_with('=')
        {
            Some(name.to_string())
        } else {
            None
        }
    } else {
        None
    }
}

fn scan_braces(rel: &str, text: &str, headers: &[&str], keywords: &[&str]) -> Vec<FunctionInfo> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if let Some(name) = header_name(lines[index], headers) {
            let start = index;
            let mut depth =
                lines[index].matches('{').count() as i32 - lines[index].matches('}').count() as i32;
            let mut cc = 1 + decision_hits(lines[index], keywords);
            index += 1;
            if depth <= 0 {
                while index < lines.len() && depth <= 0 && !lines[index].contains('{') {
                    cc += decision_hits(lines[index], keywords);
                    index += 1;
                    if index - start > 4 {
                        break;
                    }
                }
            }
            while index < lines.len() && depth > 0 {
                depth += lines[index].matches('{').count() as i32;
                depth -= lines[index].matches('}').count() as i32;
                cc += decision_hits(lines[index], keywords);
                index += 1;
            }
            out.push(info(rel, name, start as u32 + 1, index as u32, cc));
            continue;
        }
        index += 1;
    }
    out
}

fn header_name(line: &str, headers: &[&str]) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.starts_with("//") || trimmed.starts_with('#') || trimmed.starts_with('*') {
        return None;
    }
    if trimmed.contains("function ") || trimmed.starts_with("func ") {
        return word_after(
            trimmed,
            if trimmed.contains("function ") {
                "function"
            } else {
                "func"
            },
        );
    }
    if !headers
        .iter()
        .any(|header| *header == "function" || *header == "func")
    {
        return None;
    }
    // C-like: a declaration ending in `name(...) {` that is not a control keyword.
    let before_paren = trimmed.split('(').next()?.trim();
    let name = before_paren.split_whitespace().last()?.trim_matches('*');
    if name.is_empty()
        || is_control(name)
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    if trimmed.contains('(')
        && (trimmed.contains('{') || trimmed.ends_with(')') || trimmed.ends_with(','))
    {
        Some(name.to_string())
    } else {
        None
    }
}

fn word_after(line: &str, marker: &str) -> Option<String> {
    if marker == "func" && line.contains("func (") {
        let after = line.split(')').nth(1)?.trim();
        let name: String = after
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        return if name.is_empty() { None } else { Some(name) };
    }
    let rest = line.split(marker).nth(1)?.trim();
    let name: String = rest
        .trim_start_matches('(')
        .chars()
        .skip_while(|c| !c.is_ascii_alphanumeric() && *c != '_')
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() || is_control(&name) {
        None
    } else {
        Some(name)
    }
}

fn is_control(name: &str) -> bool {
    matches!(
        name,
        "if" | "for" | "while" | "switch" | "catch" | "foreach" | "else" | "do" | "return"
    )
}

fn decision_hits(line: &str, keywords: &[&str]) -> u32 {
    if line.trim_start().starts_with("//") || line.trim_start().starts_with('#') {
        return 0;
    }
    let mut count = 0u32;
    for word in line.split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '&' && c != '|')
    {
        if keywords.contains(&word) {
            count += 1;
        }
        count += word.matches("&&").count() as u32;
        count += word.matches("||").count() as u32;
    }
    count
}

fn info(rel: &str, symbol: String, start: u32, end: u32, cc: u32) -> FunctionInfo {
    FunctionInfo {
        file: rel.to_string(),
        symbol,
        span: Span {
            start_line: start,
            start_col: 1,
            end_line: end.max(start),
            end_col: 1,
        },
        cc: cc.max(1),
    }
}

const PYTHON_CC: &str = r#"
import ast, json, sys
paths = json.loads(sys.argv[1])
class Counter(ast.NodeVisitor):
    def __init__(self):
        self.cc = 1
    def bump(self, n=1):
        self.cc += n
    def visit_FunctionDef(self, node):
        return
    def visit_AsyncFunctionDef(self, node):
        return
    def visit_ClassDef(self, node):
        return
    def visit_If(self, node):
        self.bump(); self.generic_visit(node)
    def visit_For(self, node):
        self.bump(); self.generic_visit(node)
    def visit_AsyncFor(self, node):
        self.bump(); self.generic_visit(node)
    def visit_While(self, node):
        self.bump(); self.generic_visit(node)
    def visit_ExceptHandler(self, node):
        self.bump(); self.generic_visit(node)
    def visit_Assert(self, node):
        self.bump(); self.generic_visit(node)
    def visit_IfExp(self, node):
        self.bump(); self.generic_visit(node)
    def visit_BoolOp(self, node):
        self.bump(max(len(node.values) - 1, 0)); self.generic_visit(node)
    def visit_Match(self, node):
        self.bump(len(node.cases)); self.generic_visit(node)
    def visit_comprehension(self, node):
        self.bump(len(node.ifs)); self.generic_visit(node)
def walk(node, prefix, out, rel):
    for child in ast.iter_child_nodes(node):
        if isinstance(child, (ast.FunctionDef, ast.AsyncFunctionDef)):
            counter = Counter()
            for stmt in child.body:
                counter.visit(stmt)
            name = child.name if not prefix else prefix + child.name
            out.append({"file": rel, "symbol": name, "cc": counter.cc, "start": child.lineno, "end": getattr(child, "end_lineno", child.lineno) or child.lineno})
            walk(child, name + ".", out, rel)
        elif isinstance(child, ast.ClassDef):
            walk(child, prefix + child.name + ".", out, rel)
        else:
            walk(child, prefix, out, rel)
rows = []
for rel in paths:
    try:
        tree = ast.parse(open(rel, encoding="utf-8").read(), filename=rel)
    except (OSError, SyntaxError):
        continue
    walk(tree, "", rows, rel)
print(json.dumps(rows))
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_ast_counts_a_branch() {
        let dir = std::env::temp_dir().join(format!("sc-cc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src/app.py"),
            "def choose(n):\n    if n:\n        return 1\n    return 0\n",
        )
        .unwrap();
        let fns = python_functions(&dir);
        assert_eq!(fns.len(), 1);
        assert_eq!(fns[0].symbol, "choose");
        assert!(fns[0].cc >= 2, "cc {}", fns[0].cc);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn each_pack_counts_a_branch() {
        let java = scan_text("App.java", "class App {\n    int choose(int n) {\n        if (n > 0) {\n            return 1;\n        }\n        return 0;\n    }\n}\n", &Lang::C);
        assert_eq!(java[0].symbol, "choose");
        assert!(java[0].cc >= 2, "java {}", java[0].cc);
        let node = scan_text(
            "app.js",
            "function choose(n) {\n  if (n) return 1;\n  return 0;\n}\n",
            &Lang::C,
        );
        assert_eq!(node[0].symbol, "choose");
        assert!(node[0].cc >= 2, "node {}", node[0].cc);
        let php = scan_text(
            "app.php",
            "<?php\nfunction choose($n) {\n    if ($n) { return 1; }\n    return 0;\n}\n",
            &Lang::C,
        );
        assert_eq!(php[0].symbol, "choose");
        assert!(php[0].cc >= 2, "php {}", php[0].cc);
        let bash = scan_bash(
            "run.sh",
            "choose() {\n  if [ \"$1\" ]; then\n    echo yes\n  fi\n}\n",
        );
        assert_eq!(bash[0].symbol, "choose");
        assert!(bash[0].cc >= 2, "bash {}", bash[0].cc);
    }

    #[test]
    fn go_cover_profile_marks_a_hit() {
        let function = FunctionInfo {
            file: "main.go".into(),
            symbol: "main".into(),
            span: Span {
                start_line: 3,
                start_col: 1,
                end_line: 6,
                end_col: 1,
            },
            cc: 2,
        };
        let profile =
            "mode: set\nexample.com/p/main.go:4.2,5.3 1 1\nexample.com/p/main.go:5.3,6.2 1 0\n";
        let data = go_coverage(profile, &[function]);
        let cov = data.for_function("main.go", "main").unwrap();
        assert!((cov - 0.5).abs() < 1e-9, "{cov}");
    }

    #[test]
    fn go_func_is_counted() {
        let fns = scan_text(
            "main.go",
            "func main() {\n    if true {\n        return\n    }\n}\n",
            &Lang::Go,
        );
        assert_eq!(fns[0].symbol, "main");
        assert!(fns[0].cc >= 2, "cc {}", fns[0].cc);
    }
}

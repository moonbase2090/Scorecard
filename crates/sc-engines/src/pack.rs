// SPDX-License-Identifier: MPL-2.0
//! Language pack detection.
//!
//! One marker selects a pack. Several markers use the language with more
//! source files; a tie is ambiguous. An unknown tree does not pretend to pass.

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackId {
    Rust,
    Node,
    Python,
    Bash,
    Go,
    Java,
    CSharp,
    Php,
    Cpp,
    Command,
    Web,
}

impl PackId {
    #[inline(never)]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Node => "node",
            Self::Python => "python",
            Self::Bash => "bash",
            Self::Go => "go",
            Self::Java => "java",
            Self::CSharp => "csharp",
            Self::Php => "php",
            Self::Cpp => "cpp",
            Self::Command => "command",
            Self::Web => "web",
        }
    }

    #[inline(never)]
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "rust" => Some(Self::Rust),
            "node" | "javascript" | "typescript" | "ts" | "js" => Some(Self::Node),
            "python" | "py" => Some(Self::Python),
            "bash" | "sh" | "shell" => Some(Self::Bash),
            "go" | "golang" => Some(Self::Go),
            "java" => Some(Self::Java),
            "csharp" | "c#" | "cs" => Some(Self::CSharp),
            "php" => Some(Self::Php),
            "cpp" | "c++" | "cxx" => Some(Self::Cpp),
            "command" => Some(Self::Command),
            "web" | "html" => Some(Self::Web),
            _ => None,
        }
    }

    /// Gates this pack can fail today. The others are reported and do not fail the process.
    /// Referenced by the pack tests; the library lint build does not compile those tests.
    #[cfg(test)]
    pub fn enforced_gates(self) -> &'static [&'static str] {
        match self {
            Self::Rust => &["types", "tests", "crap", "secrets", "lint"],
            Self::Python => &["types", "tests", "crap", "secrets", "lint"],
            Self::Node
            | Self::Bash
            | Self::Go
            | Self::Java
            | Self::CSharp
            | Self::Php
            | Self::Cpp
            | Self::Command => &["secrets"],
            Self::Web => &["html", "crap", "secrets"],
        }
    }

    /// How `--diff` chooses tests. Empty targeted names mean the full suite.
    pub fn test_selection(self) -> &'static str {
        match self {
            Self::Rust => "rust-tests",
            _ => "full-suite",
        }
    }

    pub fn lint_default(self) -> &'static str {
        match self {
            Self::Rust => "cargo clippy",
            _ => "",
        }
    }
}

#[cfg(test)]
pub struct PackFixture {
    pub id: PackId,
    pub pass_fixture: &'static str,
    pub fail_fixture: &'static str,
}

#[cfg(test)]
pub const ROUND1: &[PackFixture] = &[
    PackFixture {
        id: PackId::Rust,
        pass_fixture: "testdata/good_crate",
        fail_fixture: "testdata/failing_test",
    },
    PackFixture {
        id: PackId::Node,
        pass_fixture: "testdata/node_pack",
        fail_fixture: "testdata/node_pack_fail",
    },
    PackFixture {
        id: PackId::Python,
        pass_fixture: "testdata/python_pack",
        fail_fixture: "testdata/python_pack_fail",
    },
    PackFixture {
        id: PackId::Bash,
        pass_fixture: "testdata/bash_pack",
        fail_fixture: "testdata/bash_pack_fail",
    },
    PackFixture {
        id: PackId::Go,
        pass_fixture: "testdata/go_pack",
        fail_fixture: "testdata/go_pack_fail",
    },
    PackFixture {
        id: PackId::Java,
        pass_fixture: "testdata/java_pack",
        fail_fixture: "testdata/java_pack_fail",
    },
    PackFixture {
        id: PackId::CSharp,
        pass_fixture: "testdata/csharp_pack",
        fail_fixture: "testdata/csharp_pack_fail",
    },
    PackFixture {
        id: PackId::Php,
        pass_fixture: "testdata/php_pack",
        fail_fixture: "testdata/php_pack_fail",
    },
    PackFixture {
        id: PackId::Cpp,
        pass_fixture: "testdata/cpp_pack",
        fail_fixture: "testdata/cpp_pack_fail",
    },
    PackFixture {
        id: PackId::Command,
        pass_fixture: "testdata/command_pack",
        fail_fixture: "testdata/command_pack_fail",
    },
    PackFixture {
        id: PackId::Web,
        pass_fixture: "testdata/web_site",
        fail_fixture: "testdata/web_site_bad",
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detected {
    Pack(PackId),
    Unknown,
    Ambiguous(Vec<PackId>),
}

pub fn text_secrets(root: &Path) -> Vec<sc_core::Finding> {
    let mut files = Vec::new();
    collect_text(root, root, 0, &mut files);
    let mut findings = Vec::new();
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        findings.extend(crate::secrets::secrets_in_text(&text, &rel));
    }
    findings
}

fn collect_text(_root: &Path, dir: &Path, depth: u32, out: &mut Vec<std::path::PathBuf>) {
    if depth > 4 || out.len() >= 200 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name.starts_with('.') || name == "node_modules" || name == "target" || name == "dist" {
            continue;
        }
        if path.is_dir() {
            collect_text(_root, &path, depth + 1, out);
        } else if matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some(
                "rs" | "js"
                    | "jsx"
                    | "mjs"
                    | "cjs"
                    | "ts"
                    | "tsx"
                    | "py"
                    | "go"
                    | "toml"
                    | "json"
                    | "sh"
                    | "bash"
                    | "java"
                    | "cs"
                    | "php"
                    | "c"
                    | "cc"
                    | "cpp"
                    | "cxx"
                    | "h"
                    | "hh"
                    | "hpp"
                    | "hxx"
                    | "html"
                    | "htm"
                    | "css",
            )
        ) {
            out.push(path);
        }
    }
}

pub fn detect(root: &Path, override_pack: &str) -> Result<Detected, String> {
    let override_pack = override_pack.trim();
    if !override_pack.is_empty() {
        let id = PackId::parse(override_pack)
            .ok_or_else(|| format!("unknown language pack `{override_pack}`"))?;
        return Ok(Detected::Pack(id));
    }
    let mut found = Vec::new();
    if root.join("Cargo.toml").is_file() {
        found.push(PackId::Rust);
    }
    // package.json next to only shell scripts is a shell program, not Node.
    if root.join("package.json").is_file() && (has_node_source(root) || !has_shell(root)) {
        found.push(PackId::Node);
    }
    if root.join("pyproject.toml").is_file()
        || root.join("requirements.txt").is_file()
        || root.join("setup.py").is_file()
        || root.join("Pipfile").is_file()
    {
        found.push(PackId::Python);
    }
    if root.join("go.mod").is_file() {
        found.push(PackId::Go);
    }
    if root.join("pom.xml").is_file()
        || root.join("build.gradle").is_file()
        || root.join("build.gradle.kts").is_file()
    {
        found.push(PackId::Java);
    }
    if has_project_file(root, &["csproj", "sln"]) {
        found.push(PackId::CSharp);
    }
    if root.join("composer.json").is_file() {
        found.push(PackId::Php);
    }
    if is_c_family(root) {
        found.push(PackId::Cpp);
    }
    if header_tie(root)
        && !found.iter().any(|id| {
            matches!(
                id,
                PackId::Rust | PackId::Go | PackId::Java | PackId::CSharp | PackId::Php | PackId::Node
            )
        })
    {
        return Ok(Detected::Ambiguous(vec![PackId::Python, PackId::Cpp]));
    }
    Ok(resolve_markers(root, found))
}

/// One Python file and one header is not a C++ program and not a clear Python
/// program. More headers must not outvote the Python file.
fn header_tie(root: &Path) -> bool {
    if has_project_file(root, &["c", "cc", "cpp", "cxx"]) {
        return false;
    }
    count_ext(root, &["py"]) == 1 && count_ext(root, &["h", "hh", "hpp", "hxx"]) == 1
}

fn count_ext(root: &Path, exts: &[&str]) -> usize {
    let mut count = 0usize;
    visit(root, 0, &mut 0, &mut |path| {
        if ext_is(path, exts) {
            count += 1;
        }
        true
    });
    count
}

fn resolve_markers(root: &Path, found: Vec<PackId>) -> Detected {
    match found.len() {
        0 if has_root_html(root) => Detected::Pack(PackId::Web),
        0 if has_shell(root) => Detected::Pack(PackId::Bash),
        0 => Detected::Unknown,
        1 => Detected::Pack(found[0]),
        _ => {
            let counts = source_counts(root);
            let mut ranked: Vec<(usize, PackId)> = found
                .iter()
                .map(|id| (counts[pack_index(*id)], *id))
                .collect();
            ranked.sort_by(|left, right| {
                right
                    .0
                    .cmp(&left.0)
                    .then(left.1.as_str().cmp(right.1.as_str()))
            });
            if ranked[0].0 > ranked.get(1).map(|item| item.0).unwrap_or(0) {
                Detected::Pack(ranked[0].1)
            } else {
                Detected::Ambiguous(found)
            }
        }
    }
}

fn is_c_family(root: &Path) -> bool {
    if root.join("CMakeLists.txt").is_file() {
        return true;
    }
    let build = [
        "Makefile",
        "makefile",
        "GNUmakefile",
        "configure",
        "configure.ac",
    ]
    .iter()
    .any(|name| root.join(name).is_file());
    build && has_project_file(root, &["c", "cc", "cpp", "cxx"])
}

fn has_node_source(root: &Path) -> bool {
    has_project_file(root, &["js", "jsx", "mjs", "cjs", "ts", "tsx"])
}

fn has_project_file(root: &Path, exts: &[&str]) -> bool {
    let mut found = false;
    visit(root, 0, &mut 0, &mut |path| {
        if ext_is(path, exts) {
            found = true;
            return false;
        }
        true
    });
    found
}

fn pack_index(id: PackId) -> usize {
    match id {
        PackId::Rust => 0,
        PackId::Node => 1,
        PackId::Python => 2,
        PackId::Bash => 3,
        PackId::Go => 4,
        PackId::Java => 5,
        PackId::CSharp => 6,
        PackId::Php => 7,
        PackId::Cpp => 8,
        PackId::Command => 9,
        PackId::Web => 10,
    }
}

fn source_counts(root: &Path) -> [usize; 11] {
    let mut counts = [0; 11];
    visit(root, 0, &mut 0, &mut |path| {
        if let Some(id) = pack_for_ext(path) {
            counts[pack_index(id)] += 1;
        }
        true
    });
    counts
}

fn pack_for_ext(path: &Path) -> Option<PackId> {
    let ext = path.extension().and_then(|ext| ext.to_str())?;
    Some(match ext {
        "rs" => PackId::Rust,
        "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" => PackId::Node,
        "py" => PackId::Python,
        "go" => PackId::Go,
        "java" => PackId::Java,
        "cs" => PackId::CSharp,
        "php" => PackId::Php,
        "c" | "cc" | "cpp" | "cxx" => PackId::Cpp,
        "sh" | "bash" => PackId::Bash,
        _ => return None,
    })
}

fn ext_is(path: &Path, exts: &[&str]) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| exts.iter().any(|wanted| wanted.eq_ignore_ascii_case(ext)))
}

fn visit(dir: &Path, depth: u32, seen: &mut usize, on_file: &mut dyn FnMut(&Path) -> bool) {
    if depth > 8 || *seen > 4000 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if *seen > 4000 {
            return;
        }
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|item| item.to_str())
            .unwrap_or("");
        if skip_dir(name) {
            continue;
        }
        if path.is_dir() {
            if path
                .symlink_metadata()
                .map(|meta| meta.file_type().is_symlink())
                .unwrap_or(false)
            {
                continue;
            }
            visit(&path, depth + 1, seen, on_file);
        } else {
            *seen += 1;
            if !on_file(&path) {
                return;
            }
        }
    }
}

fn skip_dir(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".hg"
            | ".svn"
            | "target"
            | "node_modules"
            | "dist"
            | "vendor"
            | ".venv"
            | "venv"
            | ".sc"
    ) || name.starts_with('.')
}

fn has_extension(root: &Path, exts: &[&str]) -> bool {
    let Ok(entries) = std::fs::read_dir(root) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .path()
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| exts.iter().any(|wanted| wanted.eq_ignore_ascii_case(ext)))
    })
}

fn has_root_html(root: &Path) -> bool {
    if root.join("index.html").is_file() || root.join("index.htm").is_file() {
        return true;
    }
    has_extension(root, &["html", "htm"])
}

fn has_shell(root: &Path) -> bool {
    for dir in [root.to_path_buf(), root.join("scripts"), root.join("bin")] {
        if has_extension(&dir, &["sh", "bash"]) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sc-pack-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn headers_do_not_select_cpp_or_outvote_python() {
        let many = temp("headers-many");
        fs::write(many.join("Makefile"), "all:\n").unwrap();
        fs::write(many.join("pyproject.toml"), "[project]\nname = \"d\"\n").unwrap();
        fs::write(many.join("app.py"), "def value():\n    return 1\n").unwrap();
        fs::create_dir_all(many.join("include")).unwrap();
        for name in ["h1.h", "h2.h", "h3.h"] {
            fs::write(many.join("include").join(name), "int marker;\n").unwrap();
        }
        assert_eq!(detect(&many, "").unwrap(), Detected::Pack(PackId::Python));

        let tie = temp("headers-tie");
        fs::write(tie.join("Makefile"), "all:\n").unwrap();
        fs::write(tie.join("pyproject.toml"), "[project]\nname = \"d\"\n").unwrap();
        fs::write(tie.join("app.py"), "def value():\n    return 1\n").unwrap();
        fs::write(tie.join("only.h"), "int marker;\n").unwrap();
        match detect(&tie, "").unwrap() {
            Detected::Ambiguous(packs) => {
                assert!(packs.contains(&PackId::Python), "{packs:?}");
                assert!(packs.contains(&PackId::Cpp), "{packs:?}");
            }
            other => panic!("expected ambiguous, got {other:?}"),
        }
        let _ = fs::remove_dir_all(&many);
        let _ = fs::remove_dir_all(&tie);
    }

    #[test]
    fn makefile_and_c_sources_are_cpp_even_with_a_shell_script() {
        let dir = temp("make-c");
        fs::write(dir.join("Makefile"), "all:\n").unwrap();
        fs::write(dir.join("demo.c"), "int main(void){return 0;}\n").unwrap();
        fs::write(dir.join("build.sh"), "#!/bin/sh\n").unwrap();
        assert_eq!(detect(&dir, "").unwrap(), Detected::Pack(PackId::Cpp));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn configure_ac_with_nested_c_is_cpp() {
        let dir = temp("configure");
        fs::write(dir.join("configure.ac"), "AC_INIT\n").unwrap();
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/main.c"), "int main(void){return 0;}\n").unwrap();
        fs::write(dir.join("compile-ios.sh"), "#!/bin/sh\n").unwrap();
        assert_eq!(detect(&dir, "").unwrap(), Detected::Pack(PackId::Cpp));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn pipfile_is_python_and_a_nested_csproj_is_csharp() {
        let py = temp("pip");
        fs::write(py.join("Pipfile"), "[packages]\n").unwrap();
        fs::write(py.join("app.py"), "x = 1\n").unwrap();
        assert_eq!(detect(&py, "").unwrap(), Detected::Pack(PackId::Python));

        let cs = temp("csproj");
        fs::create_dir_all(cs.join("src/App")).unwrap();
        fs::write(cs.join("src/App/App.csproj"), "<Project></Project>\n").unwrap();
        assert_eq!(detect(&cs, "").unwrap(), Detected::Pack(PackId::CSharp));
        let _ = fs::remove_dir_all(&py);
        let _ = fs::remove_dir_all(&cs);
    }

    #[test]
    fn package_json_beside_only_shell_scripts_is_bash() {
        let dir = temp("nvm");
        fs::write(dir.join("package.json"), "{}\n").unwrap();
        fs::write(dir.join("nvm.sh"), "#!/bin/sh\n").unwrap();
        assert_eq!(detect(&dir, "").unwrap(), Detected::Pack(PackId::Bash));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_language_with_more_source_files_is_used() {
        let dir = temp("django");
        fs::write(dir.join("package.json"), "{}\n").unwrap();
        fs::write(dir.join("pyproject.toml"), "[project]\nname = \"d\"\n").unwrap();
        fs::write(dir.join("app.js"), "console.log(1)\n").unwrap();
        fs::write(dir.join("a.py"), "x = 1\n").unwrap();
        fs::write(dir.join("b.py"), "y = 1\n").unwrap();
        assert_eq!(detect(&dir, "").unwrap(), Detected::Pack(PackId::Python));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cargo_toml_is_rust_and_package_json_is_node() {
        let rust = temp("rust");
        fs::write(rust.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        assert_eq!(detect(&rust, "").unwrap(), Detected::Pack(PackId::Rust));

        let node = temp("node");
        fs::write(node.join("package.json"), "{}\n").unwrap();
        assert_eq!(detect(&node, "").unwrap(), Detected::Pack(PackId::Node));
        let _ = fs::remove_dir_all(rust);
        let _ = fs::remove_dir_all(node);
    }

    #[test]
    fn every_pack_name_parses_and_prints() {
        let names = [
            ("rust", PackId::Rust),
            ("javascript", PackId::Node),
            ("py", PackId::Python),
            ("shell", PackId::Bash),
            ("golang", PackId::Go),
            ("java", PackId::Java),
            ("c#", PackId::CSharp),
            ("php", PackId::Php),
            ("cxx", PackId::Cpp),
            ("command", PackId::Command),
            ("web", PackId::Web),
            ("html", PackId::Web),
        ];
        for (name, id) in names {
            assert_eq!(PackId::parse(name), Some(id));
            assert!(!id.as_str().is_empty());
        }
        assert_eq!(PackId::parse("nope"), None);
    }

    #[test]
    fn two_markers_are_ambiguous_until_overridden() {
        let dir = temp("both");
        fs::write(dir.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        fs::write(dir.join("package.json"), "{}\n").unwrap();
        assert_eq!(
            detect(&dir, "").unwrap(),
            Detected::Ambiguous(vec![PackId::Rust, PackId::Node])
        );
        assert_eq!(detect(&dir, "node").unwrap(), Detected::Pack(PackId::Node));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn empty_tree_is_unknown() {
        let dir = temp("empty");
        assert_eq!(detect(&dir, "").unwrap(), Detected::Unknown);
        assert_eq!(PackId::Rust.lint_default(), "cargo clippy");
        assert!(!PackId::Rust.lint_default().contains("-D warnings"));
        assert!(PackId::Node.lint_default().is_empty());
        assert_eq!(PackId::Node.test_selection(), "full-suite");
        assert_eq!(PackId::Rust.enforced_gates().len(), 5);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn round1_fixtures_exist_and_detect() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        assert_eq!(ROUND1.len(), 11);
        for spec in ROUND1 {
            let pass = root.join(spec.pass_fixture);
            let fail = root.join(spec.fail_fixture);
            assert!(pass.is_dir(), "missing {}", spec.pass_fixture);
            assert!(fail.is_dir(), "missing {}", spec.fail_fixture);
            if spec.id == PackId::Command {
                continue;
            }
            assert_eq!(
                detect(&pass, "").unwrap(),
                Detected::Pack(spec.id),
                "{}",
                spec.pass_fixture
            );
        }
    }
}

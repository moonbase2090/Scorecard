//! Language pack detection.
//!
//! One marker selects a pack. Several markers and no `pack` override is
//! ambiguous. An unknown tree does not pretend to pass.

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
}

impl PackId {
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
        }
    }

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
            _ => None,
        }
    }

    /// Gates this pack can fail today. The others are reported and do not fail the process.
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
            Self::Rust => "cargo clippy -- -D warnings",
            _ => "",
        }
    }
}

pub struct PackFixture {
    pub id: PackId,
    pub pass_fixture: &'static str,
    pub fail_fixture: &'static str,
}

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

fn collect_text(root: &Path, dir: &Path, depth: u32, out: &mut Vec<std::path::PathBuf>) {
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
            collect_text(root, &path, depth + 1, out);
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
                    | "hxx",
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
    if root.join("package.json").is_file() {
        found.push(PackId::Node);
    }
    if root.join("pyproject.toml").is_file()
        || root.join("requirements.txt").is_file()
        || root.join("setup.py").is_file()
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
    if has_extension(root, &["csproj", "sln"]) {
        found.push(PackId::CSharp);
    }
    if root.join("composer.json").is_file() {
        found.push(PackId::Php);
    }
    if root.join("CMakeLists.txt").is_file() {
        found.push(PackId::Cpp);
    }
    match found.len() {
        0 if has_shell(root) => Ok(Detected::Pack(PackId::Bash)),
        0 => Ok(Detected::Unknown),
        1 => Ok(Detected::Pack(found[0])),
        _ => Ok(Detected::Ambiguous(found)),
    }
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
        assert_eq!(PackId::Rust.lint_default(), "cargo clippy -- -D warnings");
        assert!(PackId::Node.lint_default().is_empty());
        assert_eq!(PackId::Node.test_selection(), "full-suite");
        assert_eq!(PackId::Rust.enforced_gates().len(), 5);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn round1_fixtures_exist_and_detect() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        assert_eq!(ROUND1.len(), 10);
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

// SPDX-License-Identifier: MPL-2.0
//! Language pack detection.
//!
//! One marker selects a pack. Several markers stay ambiguous unless only one
//! of them has source files. An unknown tree does not pretend to pass.

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
            Self::Rust => "cargo clippy --workspace",
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

/// Files larger than this are not read. The gate says the scan was partial.
const MAX_SECRET_BYTES: u64 = 1024 * 1024;
/// A NUL in this prefix means the file is binary and is not a text secret.
const BINARY_PROBE: usize = 8192;

pub fn text_secrets(root: &Path, exclude: &[String]) -> Vec<sc_core::Finding> {
    let mut files = Vec::new();
    let mut seen_dirs = std::collections::HashSet::new();
    let mut seen_files = std::collections::HashSet::new();
    collect_text(
        root,
        root,
        exclude,
        &mut files,
        &mut seen_dirs,
        &mut seen_files,
    );
    let mut findings = Vec::new();
    let mut oversized = Vec::new();
    for path in files {
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        match path.metadata() {
            Ok(meta) if meta.len() > MAX_SECRET_BYTES => {
                // A Go build writes a binary named after the package. That file
                // is large and starts with a NUL. It is not a partial text scan.
                match binary_prefix(&path) {
                    Ok(true) => continue,
                    Ok(false) => oversized.push(rel),
                    Err(err) => findings.push(unreadable(&rel, &err)),
                }
                continue;
            }
            Ok(_) => {}
            Err(err) => {
                findings.push(unreadable(&rel, &err));
                continue;
            }
        }
        // `read_to_string` skips the whole file on one non-UTF-8 byte, which
        // hides every secret in it. Scan the bytes we can read.
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) => {
                findings.push(unreadable(&rel, &err));
                continue;
            }
        };
        if bytes.iter().take(BINARY_PROBE).any(|byte| *byte == 0) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        findings.extend(crate::secrets::secrets_in_text(&text, &rel));
    }
    if !oversized.is_empty() {
        findings.push(partial_scan(&oversized));
    }
    findings
}

fn unreadable(rel: &str, err: &std::io::Error) -> sc_core::Finding {
    sc_core::Finding {
        id: format!("secrets:unreadable:{rel}"),
        rule: "secrets.unreadable".into(),
        engine: "secrets".into(),
        severity: "error".into(),
        file: rel.to_string(),
        span: None,
        symbol: None,
        message: format!(
            "could not read {rel} ({err}), so the secrets scan does not pass. Restore read access, or exclude it with `exclude = [\"{rel}\"]` under `[scope]` in analyzer.toml."
        ),
        evidence: serde_json::json!({ "error": err.to_string() }),
        suggested_action: Some(
            "Restore read access, or add the path to scope.exclude in analyzer.toml".into(),
        ),
        disposition: String::new(),
    }
}

fn binary_prefix(path: &std::path::Path) -> std::io::Result<bool> {
    let mut file = std::fs::File::open(path)?;
    let mut buf = [0u8; BINARY_PROBE];
    let n = std::io::Read::read(&mut file, &mut buf)?;
    Ok(buf[..n].contains(&0))
}

fn partial_scan(paths: &[String]) -> sc_core::Finding {
    let first = &paths[0];
    let message = if paths.len() == 1 {
        format!(
            "skipped {first} because it is over 1 MiB, so the secrets scan was partial and does not pass. Exclude it with `exclude = [\"{first}\"]` under `[scope]` in analyzer.toml."
        )
    } else {
        format!(
            "skipped {} files over 1 MiB, including {first}, so the secrets scan was partial and does not pass. Exclude them with `exclude = [\"{first}\"]` under `[scope]` in analyzer.toml.",
            paths.len()
        )
    };
    sc_core::Finding {
        id: format!("secrets:partial:{first}"),
        rule: "secrets.partial".into(),
        engine: "secrets".into(),
        severity: "error".into(),
        file: first.clone(),
        span: None,
        symbol: None,
        message,
        evidence: serde_json::json!({ "files": paths, "limit_bytes": MAX_SECRET_BYTES }),
        suggested_action: Some(
            "Exclude the large file under [scope] in analyzer.toml, or move the secret out of it"
                .into(),
        ),
        disposition: String::new(),
    }
}

fn collect_text(
    root: &Path,
    dir: &Path,
    exclude: &[String],
    out: &mut Vec<std::path::PathBuf>,
    seen_dirs: &mut std::collections::HashSet<std::path::PathBuf>,
    seen_files: &mut std::collections::HashSet<std::path::PathBuf>,
) {
    let dir_key = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    if !seen_dirs.insert(dir_key) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let meta = match path.symlink_metadata() {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        // A directory symlink can loop, or walk the same tree twice, once the
        // depth cap is gone.
        if meta.file_type().is_symlink() && path.is_dir() {
            continue;
        }
        if path.is_dir() {
            if name == "cache" && dir.file_name().and_then(|part| part.to_str()) == Some(".yarn") {
                continue;
            }
            if skip_dir(name, dir == root) {
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if sc_graph::is_excluded(&rel, exclude) {
                continue;
            }
            collect_text(root, &path, exclude, out, seen_dirs, seen_files);
        } else {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if sc_graph::is_excluded(&rel, exclude) {
                continue;
            }
            let file_key = path.canonicalize().unwrap_or_else(|_| path.clone());
            if !seen_files.insert(file_key) {
                continue;
            }
            out.push(path);
        }
    }
}

fn skip_dir(name: &str, at_root: bool) -> bool {
    matches!(
        name,
        ".git"
            | ".sc"
            | ".venv"
            | "venv"
            | "node_modules"
            | ".tox"
            | ".mypy_cache"
            | ".pytest_cache"
            | ".next"
            | ".nuxt"
            | ".cache"
            | ".gradle"
    ) || (at_root && matches!(name, "target" | "dist"))
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
    Ok(resolve_markers(root, found))
}

fn resolve_markers(root: &Path, found: Vec<PackId>) -> Detected {
    match found.len() {
        0 if has_root_html(root) => Detected::Pack(PackId::Web),
        0 if has_shell(root) => Detected::Pack(PackId::Bash),
        0 => Detected::Unknown,
        1 => Detected::Pack(found[0]),
        _ => {
            // A file count must not choose a pack. One language owns the tree
            // only when every other marker has no source files.
            let counts = source_counts(root);
            let owners: Vec<PackId> = found
                .iter()
                .copied()
                .filter(|id| counts[pack_index(*id)] > 0)
                .collect();
            match owners.as_slice() {
                [only] => Detected::Pack(*only),
                _ => Detected::Ambiguous(found),
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

    #[test]
    fn secrets_walk_reaches_deep_files_dotenv_and_honors_exclude() {
        let key = format!("AKIA{}", "IOSFODNN7EXAMPLE");
        let root = temp("secrets-walk");
        let deep = root.join("n/n/n/n/n");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("leak.py"), format!("KEY = \"{key}\"\n")).unwrap();
        fs::write(root.join(".env"), format!("AWS_ACCESS_KEY_ID={key}\n")).unwrap();
        fs::write(root.join("README.md"), format!("key {key}\n")).unwrap();
        fs::write(root.join("secrets.yaml"), format!("key: \"{key}\"\n")).unwrap();
        let begin = "-----BEGIN ";
        let end = "PRIVATE KEY-----";
        fs::write(root.join("key.pem"), format!("{begin}RSA {end}\nMIIB\n")).unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(
            root.join("tests/leak.rs"),
            format!("const K: &str = \"{key}\";\n"),
        )
        .unwrap();
        fs::create_dir_all(root.join("a")).unwrap();
        for index in 0..200 {
            fs::write(root.join(format!("a/f{index:03}.json")), "{}\n").unwrap();
        }
        fs::create_dir_all(root.join("b")).unwrap();
        fs::write(root.join("b/leak.py"), format!("KEY = \"{key}\"\n")).unwrap();
        fs::create_dir_all(root.join("vendor")).unwrap();
        fs::write(root.join("vendor/leak.py"), format!("KEY = \"{key}\"\n")).unwrap();

        let findings = text_secrets(&root, &["vendor/**".into()]);
        let files: Vec<&str> = findings
            .iter()
            .map(|finding| finding.file.as_str())
            .collect();
        assert!(
            files.iter().any(|file| file.ends_with("n/n/n/n/n/leak.py")),
            "{files:?}"
        );
        assert!(files.iter().any(|file| file.ends_with(".env")), "{files:?}");
        assert!(
            files.iter().any(|file| file.ends_with("README.md")),
            "{files:?}"
        );
        assert!(
            files.iter().any(|file| file.ends_with("secrets.yaml")),
            "{files:?}"
        );
        assert!(
            files.iter().any(|file| file.ends_with("key.pem")),
            "{files:?}"
        );
        assert!(
            files.iter().any(|file| file.ends_with("tests/leak.rs")),
            "{files:?}"
        );
        assert!(
            files.iter().any(|file| file.ends_with("b/leak.py")),
            "{files:?}"
        );
        assert!(
            !files.iter().any(|file| file.contains("vendor/")),
            "{files:?}"
        );

        fs::create_dir_all(root.join(".venv/botocore")).unwrap();
        fs::write(
            root.join(".venv/botocore/example.json"),
            format!("\"{key}\"\n"),
        )
        .unwrap();
        fs::create_dir_all(root.join("venv")).unwrap();
        fs::write(root.join("venv/leak.py"), format!("KEY = \"{key}\"\n")).unwrap();
        fs::create_dir_all(root.join(".hidden")).unwrap();
        fs::write(root.join(".hidden/leak.py"), format!("KEY = \"{key}\"\n")).unwrap();
        fs::create_dir_all(root.join(".github/workflows")).unwrap();
        fs::write(
            root.join(".github/workflows/ci.yml"),
            format!("KEY: \"{key}\"\n"),
        )
        .unwrap();
        let outside = temp("secrets-link-target");
        fs::write(outside.join("leak.py"), format!("KEY = \"{key}\"\n")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("linked")).unwrap();

        let findings = text_secrets(&root, &["vendor/**".into()]);
        let files: Vec<&str> = findings
            .iter()
            .map(|finding| finding.file.as_str())
            .collect();
        assert!(files.iter().any(|file| file.ends_with(".env")), "{files:?}");
        assert!(
            files
                .iter()
                .any(|file| file.ends_with(".github/workflows/ci.yml")),
            "{files:?}"
        );
        assert!(
            !files.iter().any(|file| file.contains(".venv/")),
            "{files:?}"
        );
        assert!(
            !files.iter().any(|file| file.contains("venv/")),
            "{files:?}"
        );
        assert!(
            files.iter().any(|file| file.contains(".hidden/")),
            "{files:?}"
        );
        assert!(
            !files.iter().any(|file| file.contains("linked/")),
            "{files:?}"
        );
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn a_non_utf8_byte_does_not_hide_a_secret() {
        let key = format!("AKIA{}", "IOSFODNN7EXAMPLE");
        let root = temp("secrets-utf8");
        let mut bytes = key.into_bytes();
        bytes.push(0xff);
        bytes.extend(b"\n");
        fs::write(root.join("leak.py"), bytes).unwrap();
        let findings = text_secrets(&root, &[]);
        assert!(
            findings.iter().any(
                |finding| finding.file == "leak.py" && finding.rule == "secrets.aws_access_key"
            ),
            "{findings:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn common_secret_locations_are_read_and_a_large_file_is_partial() {
        let key = format!("AKIA{}", "IOSFODNN7EXAMPLE");
        let root = temp("secrets-places");
        let begin = "-----BEGIN ";
        let end = "PRIVATE KEY-----";
        fs::write(root.join("id_rsa"), format!("{begin}OPENSSH {end}\n")).unwrap();
        fs::create_dir_all(root.join(".circleci")).unwrap();
        fs::write(root.join(".circleci/config.yml"), format!("aws: {key}\n")).unwrap();
        fs::create_dir_all(root.join("src/target")).unwrap();
        fs::write(
            root.join("src/target/keys.py"),
            format!("KEY = \"{key}\"\n"),
        )
        .unwrap();
        fs::create_dir_all(root.join("target")).unwrap();
        fs::write(root.join("target/keys.py"), format!("KEY = \"{key}\"\n")).unwrap();
        fs::write(root.join(".npmrc"), format!("token={key}\n")).unwrap();
        fs::write(
            root.join("big.bin"),
            vec![b'a'; MAX_SECRET_BYTES as usize + 1],
        )
        .unwrap();
        let mut large_binary = vec![0u8; MAX_SECRET_BYTES as usize + 1];
        large_binary[1] = b'A';
        fs::write(root.join("go_pack"), large_binary).unwrap();
        let mut hidden = format!("{key}\n").into_bytes();
        hidden.insert(0, 0);
        fs::write(root.join("blob.dat"), hidden).unwrap();
        fs::create_dir_all(root.join("testdata")).unwrap();
        fs::write(root.join("testdata/leak.py"), format!("KEY = \"{key}\"\n")).unwrap();

        let findings = text_secrets(&root, &["testdata/**".into()]);
        let files: Vec<&str> = findings
            .iter()
            .map(|finding| finding.file.as_str())
            .collect();
        assert!(files.contains(&"id_rsa"), "{files:?}");
        assert!(files.contains(&".circleci/config.yml"), "{files:?}");
        assert!(files.contains(&"src/target/keys.py"), "{files:?}");
        assert!(files.contains(&".npmrc"), "{files:?}");
        assert!(!files.contains(&"target/keys.py"), "{files:?}");
        assert!(!files.contains(&"blob.dat"), "{files:?}");
        assert!(
            !files.iter().any(|file| file.contains("testdata/")),
            "{files:?}"
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule == "secrets.partial" && finding.file == "big.bin"),
            "{findings:?}"
        );
        assert!(
            !findings.iter().any(|finding| finding.file == "go_pack"),
            "{findings:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn symlinks_are_not_followed_and_testdata_exclude_hides_fixtures() {
        let key = format!("AKIA{}", "IOSFODNN7EXAMPLE");
        let root = temp("secrets-cycle");
        fs::create_dir_all(root.join("real")).unwrap();
        fs::write(root.join("real/leak.py"), format!("KEY = \"{key}\"\n")).unwrap();
        std::os::unix::fs::symlink(root.join("real"), root.join("link-a")).unwrap();
        std::os::unix::fs::symlink(root.join("link-a"), root.join("link-b")).unwrap();
        std::os::unix::fs::symlink(root.join("real/leak.py"), root.join("again.py")).unwrap();
        fs::create_dir_all(root.join("cycle")).unwrap();
        std::os::unix::fs::symlink(root.join("cycle"), root.join("cycle/loop")).unwrap();
        fs::create_dir_all(root.join("testdata")).unwrap();
        fs::write(root.join("testdata/leak.py"), format!("KEY = \"{key}\"\n")).unwrap();

        let findings = text_secrets(&root, &["testdata/**".into()]);
        let files: Vec<&str> = findings
            .iter()
            .map(|finding| finding.file.as_str())
            .collect();
        assert_eq!(
            files
                .iter()
                .filter(|file| file.ends_with("leak.py") || file.ends_with("again.py"))
                .count(),
            1,
            "{files:?}"
        );
        assert!(
            !files.iter().any(|file| file.contains("link-")),
            "{files:?}"
        );
        assert!(
            !files.iter().any(|file| file.contains("testdata/")),
            "{files:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

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

        // The reported tree: two Python files, three headers, a Makefile.
        let reported = temp("headers-reported");
        fs::write(reported.join("Makefile"), "all:\n").unwrap();
        fs::write(reported.join("pyproject.toml"), "[project]\nname = \"d\"\n").unwrap();
        fs::write(reported.join("app.py"), "def value():\n    return 1\n").unwrap();
        fs::create_dir_all(reported.join("tests")).unwrap();
        fs::write(
            reported.join("tests/test_app.py"),
            "def test_value():\n    assert True\n",
        )
        .unwrap();
        fs::create_dir_all(reported.join("include")).unwrap();
        for name in ["h1.h", "h2.h", "h3.h"] {
            fs::write(reported.join("include").join(name), "int marker;\n").unwrap();
        }
        assert_eq!(
            detect(&reported, "").unwrap(),
            Detected::Pack(PackId::Python)
        );

        let tie = temp("headers-tie");
        fs::write(tie.join("pyproject.toml"), "[project]\nname = \"d\"\n").unwrap();
        fs::write(tie.join("app.py"), "def value():\n    return 1\n").unwrap();
        fs::write(tie.join("ext.h"), "int marker;\n").unwrap();
        assert_eq!(detect(&tie, "").unwrap(), Detected::Pack(PackId::Python));
        fs::write(tie.join("Makefile"), "all:\n").unwrap();
        assert_eq!(detect(&tie, "").unwrap(), Detected::Pack(PackId::Python));
        let _ = fs::remove_dir_all(&many);
        let _ = fs::remove_dir_all(&reported);
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

        let mixed = temp("rust-cs");
        fs::write(mixed.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        fs::write(mixed.join("src_lib.rs"), "fn value() {}\n").unwrap();
        fs::create_dir_all(mixed.join("fixture")).unwrap();
        fs::write(mixed.join("fixture/App.csproj"), "<Project></Project>\n").unwrap();
        fs::write(mixed.join("fixture/App.cs"), "class App {}\n").unwrap();
        match detect(&mixed, "").unwrap() {
            Detected::Ambiguous(packs) => {
                assert!(packs.contains(&PackId::Rust), "{packs:?}");
                assert!(packs.contains(&PackId::CSharp), "{packs:?}");
            }
            other => panic!("expected ambiguous, got {other:?}"),
        }
        assert_eq!(
            detect(&mixed, "rust").unwrap(),
            Detected::Pack(PackId::Rust)
        );
        let _ = fs::remove_dir_all(&py);
        let _ = fs::remove_dir_all(&cs);
        let _ = fs::remove_dir_all(&mixed);
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
    fn several_manifests_stay_ambiguous_when_each_has_source() {
        let dir = temp("both-source");
        fs::write(
            dir.join("package.json"),
            "{\"scripts\":{\"test\":\"node __tests__/add.test.js\"}}\n",
        )
        .unwrap();
        fs::write(dir.join("pyproject.toml"), "[project]\nname = \"d\"\n").unwrap();
        fs::create_dir_all(dir.join("__tests__")).unwrap();
        fs::write(
            dir.join("__tests__/add.test.js"),
            "throw new Error('fail')\n",
        )
        .unwrap();
        fs::create_dir_all(dir.join("tools")).unwrap();
        fs::write(dir.join("tools/a.py"), "x = 1\n").unwrap();
        fs::write(dir.join("tools/b.py"), "y = 1\n").unwrap();
        fs::write(dir.join("tools/c.py"), "z = 1\n").unwrap();
        match detect(&dir, "").unwrap() {
            Detected::Ambiguous(packs) => {
                assert!(packs.contains(&PackId::Node), "{packs:?}");
                assert!(packs.contains(&PackId::Python), "{packs:?}");
            }
            other => panic!("expected ambiguous, got {other:?}"),
        }

        let clear = temp("py-owns");
        fs::write(clear.join("package.json"), "{}\n").unwrap();
        fs::write(clear.join("pyproject.toml"), "[project]\nname = \"d\"\n").unwrap();
        fs::write(clear.join("a.py"), "x = 1\n").unwrap();
        assert_eq!(detect(&clear, "").unwrap(), Detected::Pack(PackId::Python));
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&clear);
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
        assert_eq!(PackId::Rust.lint_default(), "cargo clippy --workspace");
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

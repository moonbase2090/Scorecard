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

/// Above this size, a file takes the large-file path (gitignore check, then
/// scan up to [`MAX_TEXT_BYTES`], or `secrets.partial` when larger).
const MAX_SECRET_BYTES: u64 = 1024 * 1024;
/// A file larger than this is not read, and the gate says the scan was partial.
const MAX_TEXT_BYTES: u64 = 64 * 1024 * 1024;

#[cfg(test)]
pub fn text_secrets(root: &Path, exclude: &[String]) -> Vec<sc_core::Finding> {
    text_secrets_with_generated(root, exclude, &[])
}

pub fn text_secrets_with_generated(
    root: &Path,
    exclude: &[String],
    include_generated: &[String],
) -> Vec<sc_core::Finding> {
    let mut files = Vec::new();
    let mut findings = Vec::new();
    if let Some(listing) = sc_graph::git_secrets_paths(root) {
        match listing {
            Ok(listing) => {
                for rel in listing.paths {
                    if !listing.tracked.contains(&rel)
                        && sc_graph::source_walk_skips_path(&rel, include_generated, true)
                    {
                        continue;
                    }
                    if sc_graph::secrets_scan_includes_path(root, &rel, exclude, include_generated)
                    {
                        files.push(root.join(&rel));
                    }
                }
            }
            Err(message) => {
                findings.push(unreadable_message(".", &message));
                return findings;
            }
        }
    } else {
        for item in sc_graph::walk_with_options(
            root,
            root,
            exclude,
            include_generated,
            None,
            sc_graph::WalkOptions {
                include_hidden_directories: true,
                root_directory_excludes: true,
            },
        ) {
            match item {
                sc_graph::WalkItem::Entry(entry) if entry.kind == sc_graph::WalkKind::File => {
                    files.push(entry.path);
                }
                sc_graph::WalkItem::Error { path, message } => {
                    let rel = path
                        .as_deref()
                        .map(|path| rel_path(root, path))
                        .unwrap_or_else(|| ".".into());
                    findings.push(unreadable_message(&rel, &message));
                }
                _ => {}
            }
        }
    }
    let mut big_text = Vec::new();
    for path in files {
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        match path.metadata() {
            Ok(meta) if meta.len() > MAX_SECRET_BYTES => {
                // Large files (lockfiles, assets, build output) share one path:
                // gitignored trees are skipped; others are scanned lossily up to
                // the text limit. NUL bytes do not skip — every secret pattern
                // is ASCII, and two NULs are too weak a binary signal.
                big_text.push((path, rel, meta.len()));
                continue;
            }
            Ok(_) => {}
            Err(err) => {
                findings.push(unreadable(&rel, &err));
                continue;
            }
        }
        scan_file(&path, &rel, &mut findings);
    }
    let mut oversized = Vec::new();
    for (path, rel, len) in big_text {
        if len > MAX_TEXT_BYTES {
            if file_starts_with_object_magic(&path) {
                findings.push(skipped_object_file(&rel));
            } else {
                oversized.push(rel);
            }
            continue;
        }
        scan_file(&path, &rel, &mut findings);
    }
    if !oversized.is_empty() {
        findings.push(partial_scan(&oversized));
    }
    findings
}

fn scan_file(path: &Path, rel: &str, findings: &mut Vec<sc_core::Finding>) {
    // `read_to_string` skips the whole file on one non-UTF-8 byte, which
    // hides every secret in it. Scan the bytes we can read.
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) => {
            findings.push(unreadable(rel, &err));
            return;
        }
    };
    if file_has_object_magic(&bytes) {
        findings.push(skipped_object_file(rel));
        return;
    }
    let text = String::from_utf8_lossy(&bytes);
    findings.extend(crate::secrets::secrets_in_text(&text, rel));
}

/// Read only a prefix large enough for PE `e_lfanew` and the PE signature.
fn file_starts_with_object_magic(path: &Path) -> bool {
    use std::io::Read;
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return false,
    };
    let mut prefix = [0u8; 512];
    let read = match file.read(&mut prefix) {
        Ok(n) => n,
        Err(_) => return false,
    };
    file_has_object_magic(&prefix[..read])
}

/// Recognized executable / archive magics. A leading NUL pair alone is not enough (#155).
fn file_has_object_magic(bytes: &[u8]) -> bool {
    if bytes.starts_with(b"\x7fELF") {
        return true;
    }
    if bytes.starts_with(b"\0asm") {
        return true;
    }
    if bytes.starts_with(b"!<arch>\n") {
        return true;
    }
    if bytes.len() >= 4 {
        let be = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let le = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        const MH_MAGIC: u32 = 0xfeed_face;
        const MH_CIGAM: u32 = 0xcefa_edfe;
        const MH_MAGIC_64: u32 = 0xfeed_facf;
        const MH_CIGAM_64: u32 = 0xcffa_edfe;
        const FAT_MAGIC: u32 = 0xcafe_babe;
        const FAT_CIGAM: u32 = 0xbebafeca;
        if matches!(
            be,
            MH_MAGIC | MH_CIGAM | MH_MAGIC_64 | MH_CIGAM_64 | FAT_MAGIC | FAT_CIGAM
        ) || matches!(
            le,
            MH_MAGIC | MH_CIGAM | MH_MAGIC_64 | MH_CIGAM_64 | FAT_MAGIC | FAT_CIGAM
        ) {
            return true;
        }
    }
    is_pe_image(bytes)
}

fn is_pe_image(bytes: &[u8]) -> bool {
    if !bytes.starts_with(b"MZ") || bytes.len() < 0x40 {
        return false;
    }
    let pe_offset = u32::from_le_bytes(bytes[0x3c..0x40].try_into().unwrap_or([0, 0, 0, 0]));
    let pe_offset = pe_offset as usize;
    bytes.len() >= pe_offset + 4 && bytes[pe_offset..pe_offset + 4] == *b"PE\0\0"
}

fn skipped_object_file(rel: &str) -> sc_core::Finding {
    sc_core::Finding {
        id: format!("secrets:skipped-object:{rel}"),
        rule: "secrets.skipped_object".into(),
        engine: "secrets".into(),
        severity: "info".into(),
        file: rel.to_string(),
        span: None,
        symbol: None,
        message: format!(
            "skipped {rel} because it looks like a binary or object file (ELF, Mach-O, PE, WebAssembly, or ar archive), so the secrets scan did not read its contents"
        ),
        evidence: serde_json::json!({ "reason": "object_magic" }),
        suggested_action: None,
        disposition: String::new(),
    }
}

fn unreadable(rel: &str, err: &std::io::Error) -> sc_core::Finding {
    unreadable_message(rel, &err.to_string())
}

fn unreadable_message(rel: &str, error: &str) -> sc_core::Finding {
    sc_core::Finding {
        id: format!("secrets:unreadable:{rel}"),
        rule: "secrets.unreadable".into(),
        engine: "secrets".into(),
        severity: "error".into(),
        file: rel.to_string(),
        span: None,
        symbol: None,
        message: format!(
            "could not read {rel} ({error}), so the secrets scan does not pass. Restore read access, or exclude it with `exclude = [\"{rel}\"]` under `[scope]` in analyzer.toml."
        ),
        evidence: serde_json::json!({ "error": error }),
        suggested_action: Some(
            "Restore read access, or add the path to scope.exclude in analyzer.toml".into(),
        ),
        disposition: String::new(),
    }
}

fn partial_scan(paths: &[String]) -> sc_core::Finding {
    let first = &paths[0];
    let message = if paths.len() == 1 {
        format!(
            "skipped {first} because it is over 64 MiB, so the secrets scan was partial and does not pass. Exclude it with `exclude = [\"{first}\"]` under `[scope]` in analyzer.toml."
        )
    } else {
        format!(
            "skipped {} files over 64 MiB, including {first}, so the secrets scan was partial and does not pass. Exclude them with `exclude = [\"{first}\"]` under `[scope]` in analyzer.toml.",
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
        evidence: serde_json::json!({ "files": paths, "limit_bytes": MAX_TEXT_BYTES }),
        suggested_action: Some(
            "Exclude the large file under [scope] in analyzer.toml, or move the secret out of it"
                .into(),
        ),
        disposition: String::new(),
    }
}

fn rel_path(root: &Path, path: &Path) -> String {
    let rel = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    if rel.is_empty() {
        ".".to_string()
    } else {
        rel
    }
}

pub fn detect_with_generated(
    root: &Path,
    override_pack: &str,
    include_generated: &[String],
) -> Result<Detected, String> {
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
    if root.join("package.json").is_file()
        && (has_node_source(root, include_generated) || !has_shell(root, include_generated))
    {
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
    if has_project_file(root, &["csproj", "sln"], include_generated) {
        found.push(PackId::CSharp);
    }
    if root.join("composer.json").is_file() {
        found.push(PackId::Php);
    }
    if is_c_family(root, include_generated) {
        found.push(PackId::Cpp);
    }
    Ok(resolve_markers(root, found, include_generated))
}

#[cfg(test)]
pub(crate) fn detect(root: &Path, override_pack: &str) -> Result<Detected, String> {
    detect_with_generated(root, override_pack, &[])
}

fn resolve_markers(root: &Path, found: Vec<PackId>, include_generated: &[String]) -> Detected {
    match found.len() {
        0 if has_root_html(root, include_generated) => Detected::Pack(PackId::Web),
        0 if has_shell(root, include_generated) => Detected::Pack(PackId::Bash),
        0 => Detected::Unknown,
        1 => Detected::Pack(found[0]),
        _ => {
            // A file count must not choose a pack. One language owns the tree
            // only when every other marker has no source files.
            let counts = source_counts(root, include_generated);
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

fn is_c_family(root: &Path, include_generated: &[String]) -> bool {
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
    build && has_project_file(root, &["c", "cc", "cpp", "cxx"], include_generated)
}

fn has_node_source(root: &Path, include_generated: &[String]) -> bool {
    has_project_file(
        root,
        &["js", "jsx", "mjs", "cjs", "ts", "tsx"],
        include_generated,
    )
}

fn has_project_file(root: &Path, exts: &[&str], include_generated: &[String]) -> bool {
    let mut found = false;
    visit(root, 8, 4000, include_generated, &mut |path| {
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

fn source_counts(root: &Path, include_generated: &[String]) -> [usize; 11] {
    let mut counts = [0; 11];
    visit(root, 8, 4000, include_generated, &mut |path| {
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

fn visit(
    root: &Path,
    max_depth: usize,
    max_files: usize,
    include_generated: &[String],
    on_file: &mut dyn FnMut(&Path) -> bool,
) {
    let mut seen = 0usize;
    for item in sc_graph::walk(root, root, &[], include_generated, Some(max_depth)) {
        if seen >= max_files {
            return;
        }
        if let sc_graph::WalkItem::Entry(entry) = item {
            if entry.kind == sc_graph::WalkKind::File {
                seen += 1;
                if !on_file(&entry.path) {
                    return;
                }
            }
        }
    }
}

fn has_extension(root: &Path, exts: &[&str], include_generated: &[String]) -> bool {
    sc_graph::walk_files(root, root, &[], include_generated, Some(1))
        .iter()
        .any(|path| ext_is(path, exts))
}

fn has_root_html(root: &Path, include_generated: &[String]) -> bool {
    has_extension(root, &["html", "htm"], include_generated)
}

fn has_shell(root: &Path, include_generated: &[String]) -> bool {
    for dir in [root.to_path_buf(), root.join("scripts"), root.join("bin")] {
        if has_extension(&dir, &["sh", "bash"], include_generated) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Not the AWS documentation id. That id ends in `EXAMPLE` and is ignored.
    fn planted_access_key() -> String {
        format!("AKIA{}{}", "0Z3VS5J4", "AB3KQM9Z")
    }

    fn planted_pem(kind: &str) -> String {
        let begin = "-----BEGIN ";
        let end = "PRIVATE KEY-----";
        let body = format!("{}{}", "MIIEowIBAAKCAQEA0Z3V", "S5J4Ab3kQm9ZnR4pLx7w");
        format!("{begin}{kind} {end} {body}\n")
    }

    #[test]
    fn secrets_walk_reaches_deep_files_dotenv_and_honors_exclude() {
        let key = planted_access_key();
        let root = temp("secrets-walk");
        let deep = root.join("n/n/n/n/n");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("leak.py"), format!("KEY = \"{key}\"\n")).unwrap();
        fs::write(root.join(".env"), format!("AWS_ACCESS_KEY_ID={key}\n")).unwrap();
        fs::write(root.join("README.md"), format!("key {key}\n")).unwrap();
        fs::write(root.join("secrets.yaml"), format!("key: \"{key}\"\n")).unwrap();
        fs::write(root.join("key.pem"), planted_pem("RSA")).unwrap();
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
        let key = planted_access_key();
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
        let key = planted_access_key();
        let root = temp("secrets-places");
        fs::write(root.join("id_rsa"), planted_pem("OPENSSH")).unwrap();
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
            root.join("Cargo.lock"),
            vec![b'a'; MAX_SECRET_BYTES as usize + 1],
        )
        .unwrap();
        let mut lock = vec![b'{'; MAX_SECRET_BYTES as usize];
        lock.extend(format!("\"{key}\"").into_bytes());
        fs::write(root.join("package-lock.json"), lock).unwrap();
        fs::write(root.join(".gitignore"), "build/\n").unwrap();
        fs::create_dir_all(root.join("build/static/js")).unwrap();
        fs::write(
            root.join("build/static/js/main.js.map"),
            vec![b'a'; MAX_SECRET_BYTES as usize + 1],
        )
        .unwrap();
        let _ = std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .arg("init")
            .status();
        let mut large_binary = vec![0u8; MAX_SECRET_BYTES as usize + 1];
        large_binary[1] = b'A';
        fs::write(root.join("go_pack"), large_binary).unwrap();
        let mut hidden = format!("{key}\n").into_bytes();
        hidden.insert(0, 0);
        fs::write(root.join("blob.dat"), hidden).unwrap();
        let mut js = format!("// note\nconst k = \"{key}\";\n").into_bytes();
        js.insert(3, 0);
        fs::write(root.join("app.js"), js).unwrap();
        fs::create_dir_all(root.join("testdata")).unwrap();
        fs::write(root.join("testdata/leak.py"), format!("KEY = \"{key}\"\n")).unwrap();

        let findings = text_secrets(
            &root,
            &[
                "target/**".into(),
                "generated/**".into(),
                "testdata/**".into(),
            ],
        );
        let files: Vec<&str> = findings
            .iter()
            .map(|finding| finding.file.as_str())
            .collect();
        assert!(files.contains(&"id_rsa"), "{files:?}");
        assert!(files.contains(&".circleci/config.yml"), "{files:?}");
        assert!(files.contains(&"src/target/keys.py"), "{files:?}");
        assert!(files.contains(&".npmrc"), "{files:?}");
        assert!(!files.contains(&"target/keys.py"), "{files:?}");
        assert!(files.contains(&"blob.dat"), "{files:?}");
        assert!(files.contains(&"app.js"), "{files:?}");
        assert!(
            !files.iter().any(|file| file.contains("testdata/")),
            "{files:?}"
        );
        assert!(
            findings.iter().any(|finding| {
                finding.file == "package-lock.json" && finding.rule == "secrets.aws_access_key"
            }),
            "{findings:?}"
        );
        assert!(
            !findings.iter().any(|finding| {
                finding.file == "Cargo.lock" || finding.file.starts_with("build/")
            }),
            "{findings:?}"
        );
        assert!(
            !findings.iter().any(|finding| finding.file == "go_pack"),
            "{findings:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn one_nul_in_a_large_file_does_not_hide_a_key() {
        let key = planted_access_key();
        let root = temp("secrets-one-nul");
        let mut bytes = vec![0u8];
        bytes.extend(format!("const AWS_KEY = \"{key}\";\n").into_bytes());
        bytes.resize(MAX_SECRET_BYTES as usize + 8, b' ');
        fs::write(root.join("config.js"), &bytes).unwrap();
        let findings = text_secrets(&root, &[]);
        assert!(
            findings.iter().any(|finding| {
                finding.file == "config.js" && finding.rule == "secrets.aws_access_key"
            }),
            "{findings:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    fn runtime_github_token() -> String {
        let parts = ["Ab", "3k", "Qm", "9Z", "nR", "4p", "Lx", "7w"];
        let tail: String = parts.iter().cycle().take(18).copied().collect();
        format!("ghp_{tail}")
    }

    fn git_commit(root: &std::path::Path) {
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(root)
                .args(args)
                .env("GIT_AUTHOR_NAME", "sc")
                .env("GIT_AUTHOR_EMAIL", "sc@example.com")
                .env("GIT_COMMITTER_NAME", "sc")
                .env("GIT_COMMITTER_EMAIL", "sc@example.com")
                .status()
                .unwrap();
            assert!(status.success(), "{args:?}");
        };
        git(&["init", "-q"]);
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "init"]);
    }

    #[test]
    fn tracked_vendor_dist_and_build_are_scanned_in_git() {
        let token = runtime_github_token();
        let root = temp("secrets-git-tracked");
        fs::create_dir_all(root.join("vendor/dep")).unwrap();
        fs::create_dir_all(root.join("dist")).unwrap();
        fs::create_dir_all(root.join("build")).unwrap();
        fs::write(
            root.join("vendor/dep/lib.js"),
            format!("const V = \"{token}\";\n"),
        )
        .unwrap();
        fs::write(root.join("dist/app.js"), format!("var k=\"{token}\";\n")).unwrap();
        fs::write(root.join("build/c.txt"), format!("k={token}\n")).unwrap();
        git_commit(&root);
        let findings = text_secrets(&root, &[]);
        for file in ["vendor/dep/lib.js", "dist/app.js", "build/c.txt"] {
            assert!(
                findings.iter().any(|finding| {
                    finding.file == file && finding.rule.starts_with("secrets.")
                }),
                "missing {file}: {findings:?}"
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn gitignored_env_and_target_are_not_scanned() {
        let token = runtime_github_token();
        let root = temp("secrets-git-ignored");
        fs::write(root.join(".gitignore"), ".env\ntarget/\nnode_modules/\n").unwrap();
        fs::write(root.join(".env"), format!("TOKEN={token}\n")).unwrap();
        fs::create_dir_all(root.join("target/debug")).unwrap();
        fs::write(root.join("target/debug/leak.txt"), format!("{token}\n")).unwrap();
        fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        fs::write(
            root.join("node_modules/pkg/index.js"),
            format!("\"{token}\"\n"),
        )
        .unwrap();
        git_commit(&root);
        let findings = text_secrets(&root, &[]);
        assert!(
            !findings.iter().any(|finding| {
                finding.file == ".env"
                    || finding.file.starts_with("target/")
                    || finding.file.starts_with("node_modules/")
            }),
            "{findings:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn scope_exclude_hides_dist_secret_in_git() {
        let token = runtime_github_token();
        let root = temp("secrets-git-exclude-dist");
        fs::create_dir_all(root.join("vendor/dep")).unwrap();
        fs::create_dir_all(root.join("dist")).unwrap();
        fs::create_dir_all(root.join("build")).unwrap();
        fs::write(
            root.join("vendor/dep/lib.js"),
            format!("const V = \"{token}\";\n"),
        )
        .unwrap();
        fs::write(root.join("dist/app.js"), format!("var k=\"{token}\";\n")).unwrap();
        fs::write(root.join("build/c.txt"), format!("k={token}\n")).unwrap();
        git_commit(&root);
        let findings = text_secrets(&root, &["dist/**".into()]);
        assert!(
            !findings
                .iter()
                .any(|finding| finding.file.starts_with("dist/")),
            "{findings:?}"
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.file == "vendor/dep/lib.js"),
            "{findings:?}"
        );
        assert!(
            findings.iter().any(|finding| finding.file == "build/c.txt"),
            "{findings:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn elf_magic_skips_lossy_scan_for_runtime_tokens() {
        let tail = ["Ab", "3k", "Qm", "9Z", "nR", "4p", "Lx", "7w"]
            .iter()
            .cycle()
            .take(18)
            .copied()
            .collect::<String>();
        let token = format!("ghp_{tail}");
        let root = temp("secrets-elf-magic");
        let mut bytes = vec![0x7f, b'E', b'L', b'F'];
        bytes.resize(4 + 64, 0);
        bytes.extend(token.as_bytes());
        fs::write(root.join("tool"), &bytes).unwrap();
        let findings = text_secrets(&root, &[]);
        assert!(
            !findings.iter().any(|finding| {
                finding.file == "tool"
                    && finding.severity == "error"
                    && finding.rule.starts_with("secrets.")
            }),
            "{findings:?}"
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.file == "tool" && finding.rule == "secrets.skipped_object"),
            "{findings:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn oversized_elf_is_skipped_not_partial() {
        let tail = "x".repeat(32);
        let token = format!("ghp_{tail}");
        let root = temp("secrets-oversized-elf");
        let mut bytes = vec![0x7f, b'E', b'L', b'F'];
        bytes.resize(MAX_TEXT_BYTES as usize + 1, 0);
        bytes.extend(token.as_bytes());
        fs::write(root.join("big"), &bytes).unwrap();
        let findings = text_secrets(&root, &[]);
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "secrets.partial"),
            "{findings:?}"
        );
        assert!(
            findings.iter().any(|finding| {
                finding.file == "big" && finding.rule == "secrets.skipped_object"
            }),
            "{findings:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn two_nuls_in_a_large_file_do_not_hide_a_key() {
        // Reproduce #155: a >1 MiB file that starts with two NULs used to be
        // skipped as "binary" with no secrets.partial and no finding.
        let key = planted_access_key();
        let root = temp("secrets-two-nul");
        let mut bytes = vec![0u8, 0u8];
        bytes.resize(1_200_000, b'a');
        bytes.extend(format!("\nK=\"{key}\"\n").into_bytes());
        fs::write(root.join("assets.bin"), &bytes).unwrap();
        let findings = text_secrets(&root, &[]);
        assert!(
            findings.iter().any(|finding| {
                finding.file == "assets.bin" && finding.rule == "secrets.aws_access_key"
            }),
            "{findings:?}"
        );
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == "secrets.partial"),
            "under 64 MiB must be scanned, not partial: {findings:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_unreadable_directory_fails_the_secrets_gate() {
        use std::os::unix::fs::PermissionsExt;
        let root = temp("secrets-unreadable");
        let hidden = root.join("private");
        fs::create_dir_all(hidden.join("nested")).unwrap();
        fs::write(hidden.join("nested/leak.py"), "x = 1\n").unwrap();
        let mut blocked = fs::metadata(&hidden).unwrap().permissions();
        blocked.set_mode(0o0);
        fs::set_permissions(&hidden, blocked).unwrap();
        let findings = text_secrets(&root, &[]);
        assert!(
            findings.iter().any(|finding| {
                finding.rule == "secrets.unreadable" && finding.file == "private"
            }),
            "{findings:?}"
        );
        let mut open = fs::metadata(&hidden).unwrap().permissions();
        open.set_mode(0o755);
        fs::set_permissions(&hidden, open).unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn partial_scan_records_the_text_limit() {
        let finding = partial_scan(&["big.bin".into()]);
        assert_eq!(finding.evidence["limit_bytes"], MAX_TEXT_BYTES);
        assert!(finding.message.contains("64 MiB"), "{}", finding.message);
    }

    #[test]
    fn symlinks_are_not_followed_and_testdata_exclude_hides_fixtures() {
        let key = planted_access_key();
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

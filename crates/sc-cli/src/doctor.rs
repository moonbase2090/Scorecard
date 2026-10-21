// SPDX-License-Identifier: MIT
//! `sc doctor`: list the tools each language pack needs and how to install the missing ones.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

enum Probe {
    /// An executable on `PATH`.
    Bin(&'static str),
    /// A command that exits 0 when the tool works.
    Run(&'static [&'static str]),
}

enum Install {
    Cmd(&'static str),
    /// A system package: `brew install` on macOS, `apt-get install` elsewhere.
    System {
        brew: &'static str,
        apt: &'static str,
    },
}

struct Tool {
    name: &'static str,
    probe: Probe,
    /// A gate that is enforced by default cannot run without it.
    required: bool,
    needed_for: &'static str,
    install: Install,
}

const fn tool(
    name: &'static str,
    probe: Probe,
    required: bool,
    needed_for: &'static str,
    install: Install,
) -> Tool {
    Tool {
        name,
        probe,
        required,
        needed_for,
        install,
    }
}

fn pack_tools(pack: &str) -> Vec<Tool> {
    use Install::{Cmd, System};
    use Probe::{Bin, Run};
    match pack {
        "rust" => vec![
            tool("cargo", Bin("cargo"), true, "types, tests", Cmd("curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh")),
            tool("clippy", Run(&["cargo", "clippy", "--version"]), true, "lint", Cmd("rustup component add clippy")),
            tool("cargo-llvm-cov", Bin("cargo-llvm-cov"), true, "coverage, crap", Cmd("rustup component add llvm-tools-preview && cargo install cargo-llvm-cov --locked")),
            tool("cargo-mutants", Bin("cargo-mutants"), false, "--mutation", Cmd("cargo install cargo-mutants --locked")),
        ],
        "python" => vec![
            tool("python3", Bin("python3"), true, "compile, tests", System { brew: "python", apt: "python3" }),
            tool("pytest", Run(&["python3", "-m", "pytest", "--version"]), true, "tests", Cmd("python3 -m pip install pytest pytest-cov")),
            tool("ruff", Bin("ruff"), true, "lint", Cmd("python3 -m pip install ruff")),
        ],
        "node" => vec![
            tool("node", Bin("node"), true, "types, tests", System { brew: "node", apt: "nodejs" }),
            tool("npm", Bin("npm"), false, "tests (npm test)", System { brew: "node", apt: "npm" }),
            tool("c8", Bin("c8"), false, "coverage, crap", Cmd("npm install --save-dev c8")),
            tool("tsc", Bin("tsc"), false, "TypeScript types", Cmd("npm install --save-dev typescript")),
            tool("eslint", Bin("eslint"), false, "lint", Cmd("npm install --save-dev eslint")),
        ],
        "go" => vec![tool("go", Bin("go"), true, "types, tests, lint", System { brew: "go", apt: "golang-go" })],
        "java" => vec![
            tool("javac", Bin("javac"), true, "types", System { brew: "openjdk", apt: "default-jdk" }),
            tool("mvn", Bin("mvn"), false, "tests, coverage (Maven)", System { brew: "maven", apt: "maven" }),
            tool("gradle", Bin("gradle"), false, "tests, coverage (Gradle)", System { brew: "gradle", apt: "gradle" }),
        ],
        "csharp" => vec![tool("dotnet", Bin("dotnet"), true, "types, tests", System { brew: "dotnet", apt: "dotnet-sdk-8.0" })],
        "php" => vec![
            tool("php", Bin("php"), true, "types", System { brew: "php", apt: "php-cli" }),
            tool("phpunit", Bin("phpunit"), false, "tests", Cmd("composer require --dev phpunit/phpunit")),
            tool("phpstan", Bin("phpstan"), false, "lint", Cmd("composer require --dev phpstan/phpstan")),
        ],
        "cpp" => vec![
            tool("cc", Bin("cc"), true, "types (C files)", System { brew: "gcc", apt: "build-essential" }),
            tool("g++", Bin("g++"), true, "types (C++ files)", System { brew: "gcc", apt: "build-essential" }),
            tool("cmake", Bin("cmake"), false, "build and tests with CMakeLists.txt", System { brew: "cmake", apt: "cmake" }),
            tool("lcov", Bin("lcov"), false, "coverage, crap", System { brew: "lcov", apt: "lcov" }),
        ],
        "bash" => vec![
            tool("bash", Bin("bash"), true, "syntax check", System { brew: "bash", apt: "bash" }),
            tool("shellcheck", Bin("shellcheck"), false, "lint", System { brew: "shellcheck", apt: "shellcheck" }),
            tool("bats", Bin("bats"), false, "tests", System { brew: "bats-core", apt: "bats" }),
            tool("kcov", Bin("kcov"), false, "coverage, crap", System { brew: "kcov", apt: "kcov" }),
        ],
        _ => Vec::new(),
    }
}

fn install_command(install: &Install, macos: bool) -> String {
    match install {
        Install::Cmd(cmd) => (*cmd).to_string(),
        Install::System { brew, .. } if macos => format!("brew install {brew}"),
        Install::System { apt, .. } => format!("sudo apt-get install -y {apt}"),
    }
}

fn on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| is_executable(&dir.join(name)))
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file() || path.with_extension("exe").is_file()
    }
}

fn present(probe: &Probe) -> bool {
    match probe {
        Probe::Bin(name) => on_path(name),
        Probe::Run(argv) => Command::new(argv[0])
            .args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success()),
    }
}

/// The report text and whether every required tool is present.
fn render(packs: &[&str], has: &dyn Fn(&Tool) -> bool, macos: bool) -> (String, bool) {
    let mut out = String::new();
    let mut ok = true;
    for pack in packs {
        let tools = pack_tools(pack);
        out.push_str(&format!("{pack} pack\n"));
        if tools.is_empty() {
            out.push_str("  no external tools needed\n");
        }
        for tool in &tools {
            if has(tool) {
                out.push_str(&format!("  ok        {}\n", tool.name));
                continue;
            }
            let (label, kind) = if tool.required {
                ok = false;
                ("MISSING", "required")
            } else {
                ("missing", "optional")
            };
            out.push_str(&format!(
                "  {label:<9} {} ({kind}, used for {})\n            install: {}\n",
                tool.name,
                tool.needed_for,
                install_command(&tool.install, macos)
            ));
        }
    }
    (out, ok)
}

pub fn run(path: Option<PathBuf>, pack: Option<String>) -> i32 {
    let path = path.unwrap_or_else(|| PathBuf::from("."));
    let packs = match sc_engines::detected_pack_names(&path, pack.as_deref().unwrap_or("")) {
        Ok(packs) => packs,
        Err(err) => {
            eprintln!("sc: {err}");
            return 2;
        }
    };
    if packs.is_empty() {
        eprintln!(
            "sc: no language pack matches {}. Pass --pack rust, node, python, bash, go, java, csharp, php, cpp, or web.",
            path.display()
        );
        return 2;
    }
    let (text, ok) = render(
        &packs,
        &|tool| present(&tool.probe),
        cfg!(target_os = "macos"),
    );
    print!("{text}");
    if ok {
        0
    } else {
        eprintln!("sc: required tools are missing. Install them with the commands above, then re-run `sc doctor`.");
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_required_tool_fails_and_prints_its_install_command() {
        let (text, ok) = render(&["rust"], &|tool| tool.name != "cargo-llvm-cov", false);
        assert!(!ok);
        assert!(
            text.contains("MISSING   cargo-llvm-cov (required, used for coverage, crap)"),
            "{text}"
        );
        assert!(
            text.contains("install: rustup component add llvm-tools-preview && cargo install cargo-llvm-cov --locked"),
            "{text}"
        );
        assert!(text.contains("ok        cargo\n"), "{text}");
    }

    #[test]
    fn missing_optional_tool_passes_with_a_lowercase_label() {
        let (text, ok) = render(&["rust"], &|tool| tool.name != "cargo-mutants", false);
        assert!(ok);
        assert!(
            text.contains("missing   cargo-mutants (optional, used for --mutation)"),
            "{text}"
        );
    }

    #[test]
    fn system_packages_use_brew_on_macos_and_apt_elsewhere() {
        let (mac, _) = render(&["bash"], &|tool| tool.name != "shellcheck", true);
        let (linux, _) = render(&["bash"], &|tool| tool.name != "shellcheck", false);
        assert!(mac.contains("install: brew install shellcheck"), "{mac}");
        assert!(
            linux.contains("install: sudo apt-get install -y shellcheck"),
            "{linux}"
        );
    }

    #[test]
    fn packs_without_external_tools_say_so() {
        let (text, ok) = render(&["web"], &|_| false, false);
        assert!(ok);
        assert_eq!(text, "web pack\n  no external tools needed\n");
    }

    #[test]
    fn bin_probe_finds_an_executable_and_rejects_a_missing_name() {
        assert!(on_path("sh"));
        assert!(!on_path("sc-no-such-tool-0b7e"));
    }
}

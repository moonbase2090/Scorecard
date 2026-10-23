// SPDX-License-Identifier: MIT
//! `sc doctor`: list the tools each language pack needs and how to install the missing ones.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

enum Probe {
    /// An executable on `PATH`.
    Bin(&'static str),
    /// A command that exits 0 when the tool works.
    Run(&'static [&'static str]),
    /// Any one of these is enough.
    Any(&'static [Probe]),
}

enum Install {
    Cmd(&'static str),
    /// A system package, installed with the host's package manager.
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

/// What the project tree changes about the tools a pack needs, read the way
/// `sc analyze` reads it.
#[derive(Default, Clone, Copy)]
struct Tree {
    /// A pytest suite exists, so the tests gate is enforced.
    pytest_suite: bool,
    /// `uv.lock` or `.venv` exists, so tests run through `uv run`.
    uv_project: bool,
    coverage_enabled: bool,
    c_files: bool,
    cxx_files: bool,
}

fn pack_tools(pack: &str, tree: Tree) -> Vec<Tool> {
    use Install::{Cmd, System};
    use Probe::{Any, Bin, Run};
    match pack {
        "rust" => vec![
            tool("cargo", Bin("cargo"), true, "types, tests", Cmd("curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh")),
            tool("clippy", Run(&["cargo", "clippy", "--version"]), true, "lint", Cmd("rustup component add clippy")),
            tool("cargo-llvm-cov", Bin("cargo-llvm-cov"), true, "coverage, crap", Cmd("rustup component add llvm-tools-preview && cargo install cargo-llvm-cov --locked")),
            tool("cargo-mutants", Bin("cargo-mutants"), false, "--mutation", Cmd("cargo install cargo-mutants --locked")),
        ],
        "python" => vec![
            tool("python3", Bin("python3"), true, "compile, tests", System { brew: "python", apt: "python3" }),
            tool(
                if tree.uv_project { "uv" } else { "pytest" },
                if tree.uv_project {
                    Bin("uv")
                } else {
                    Run(&["python3", "-m", "pytest", "--version"])
                },
                tree.pytest_suite,
                if tree.uv_project { "tests via uv run" } else if tree.pytest_suite { "tests" } else { "tests (no pytest suite found)" },
                if tree.uv_project { Cmd("python3 -m pip install uv") } else { Cmd("python3 -m pip install pytest") },
            ),
            tool(
                "pytest-cov or coverage.py",
                if tree.uv_project {
                    Bin("uv")
                } else {
                    Any(&[Run(&["python3", "-c", "import pytest_cov"]), Run(&["python3", "-m", "coverage", "--version"])])
                },
                tree.pytest_suite && tree.coverage_enabled,
                "coverage, crap",
                if tree.uv_project { Cmd("python3 -m pip install uv") } else { Cmd("python3 -m pip install pytest-cov") },
            ),
            tool("ruff", Any(&[Bin("ruff"), Bin("uv")]), true, "lint (ruff, or uvx ruff)", Cmd("python3 -m pip install ruff")),
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
            tool("cc", Bin("cc"), tree.c_files, "types (C files)", System { brew: "gcc", apt: "build-essential" }),
            tool("g++", Bin("g++"), tree.cxx_files, "types (C++ files)", System { brew: "gcc", apt: "build-essential" }),
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

#[derive(Clone, Copy, PartialEq, Debug)]
enum PackageManager {
    Brew,
    Apt,
    Other,
}

fn package_manager() -> PackageManager {
    if cfg!(target_os = "macos") {
        PackageManager::Brew
    } else if on_path("apt-get") {
        PackageManager::Apt
    } else {
        PackageManager::Other
    }
}

fn install_command(install: &Install, manager: PackageManager) -> String {
    match (install, manager) {
        (Install::Cmd(cmd), _) => (*cmd).to_string(),
        (Install::System { brew, .. }, PackageManager::Brew) => format!("brew install {brew}"),
        (Install::System { apt, .. }, PackageManager::Apt) => {
            format!("sudo apt-get install -y {apt}")
        }
        (Install::System { apt, .. }, PackageManager::Other) => {
            format!("install `{apt}` (Debian package name) with your system package manager")
        }
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
        Probe::Any(probes) => probes.iter().any(present),
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
fn render(
    packs: &[&str],
    tree: Tree,
    has: &dyn Fn(&Tool) -> bool,
    manager: PackageManager,
) -> (String, bool) {
    let mut out = String::new();
    let mut ok = true;
    for pack in packs {
        let tools = pack_tools(pack, tree);
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
                install_command(&tool.install, manager)
            ));
        }
    }
    (out, ok)
}

pub fn run(path: Option<PathBuf>, pack: Option<String>) -> i32 {
    let path = path.unwrap_or_else(|| PathBuf::from("."));
    let root = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
    let config =
        match sc_core::load_config_file(sc_core::resolve_config_path(None, &root).as_deref()) {
            Ok(config) => config,
            Err(err) => {
                eprintln!("sc: {err}");
                return 2;
            }
        };
    let chosen = pack.unwrap_or(config.pack.clone());
    let packs = match sc_engines::detected_pack_names(&root, &chosen) {
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
    if packs.len() > 1 {
        eprintln!(
            "sc: several language packs match ({}); set pack in analyzer.toml or pass --pack",
            packs.join(", ")
        );
        return 2;
    }
    let (c_files, cxx_files) = sc_engines::c_family_present(
        &root,
        &config.scope.exclude,
        &config.scope.include_generated,
    );
    let tree = Tree {
        pytest_suite: sc_engines::has_pytest_suite(&root),
        uv_project: root.join("uv.lock").is_file() || root.join(".venv").is_dir(),
        coverage_enabled: config.engines.coverage,
        c_files,
        cxx_files,
    };
    let (text, ok) = render(
        &packs,
        tree,
        &|tool| present(&tool.probe),
        package_manager(),
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
        let (text, ok) = render(
            &["rust"],
            Tree::default(),
            &|tool| tool.name != "cargo-llvm-cov",
            PackageManager::Apt,
        );
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
        let (text, ok) = render(
            &["rust"],
            Tree::default(),
            &|tool| tool.name != "cargo-mutants",
            PackageManager::Apt,
        );
        assert!(ok);
        assert!(
            text.contains("missing   cargo-mutants (optional, used for --mutation)"),
            "{text}"
        );
    }

    #[test]
    fn system_packages_use_brew_on_macos_and_apt_elsewhere() {
        let (mac, _) = render(
            &["bash"],
            Tree::default(),
            &|tool| tool.name != "shellcheck",
            PackageManager::Brew,
        );
        let (linux, _) = render(
            &["bash"],
            Tree::default(),
            &|tool| tool.name != "shellcheck",
            PackageManager::Apt,
        );
        assert!(mac.contains("install: brew install shellcheck"), "{mac}");
        assert!(
            linux.contains("install: sudo apt-get install -y shellcheck"),
            "{linux}"
        );
    }

    #[test]
    fn without_apt_or_brew_the_package_name_is_given_for_the_user_to_install() {
        let (text, _) = render(
            &["bash"],
            Tree::default(),
            &|tool| tool.name != "shellcheck",
            PackageManager::Other,
        );
        assert!(text.contains("install: install `shellcheck` (Debian package name) with your system package manager"), "{text}");
    }

    #[test]
    fn a_c_only_tree_does_not_require_gxx_and_a_cxx_only_tree_does_not_require_cc() {
        let no_gxx = |tool: &Tool| tool.name != "g++";
        let no_cc = |tool: &Tool| tool.name != "cc";
        let c_only = Tree {
            c_files: true,
            ..Tree::default()
        };
        let cxx_only = Tree {
            cxx_files: true,
            ..Tree::default()
        };
        assert!(render(&["cpp"], c_only, &no_gxx, PackageManager::Apt).1);
        assert!(!render(&["cpp"], c_only, &no_cc, PackageManager::Apt).1);
        assert!(render(&["cpp"], cxx_only, &no_cc, PackageManager::Apt).1);
        assert!(!render(&["cpp"], cxx_only, &no_gxx, PackageManager::Apt).1);
    }

    #[test]
    fn pytest_is_required_only_when_the_tree_has_a_suite() {
        let no_pytest = |tool: &Tool| tool.name != "pytest";
        assert!(
            render(
                &["python"],
                Tree::default(),
                &no_pytest,
                PackageManager::Apt
            )
            .1
        );
        let suite = Tree {
            pytest_suite: true,
            ..Tree::default()
        };
        assert!(!render(&["python"], suite, &no_pytest, PackageManager::Apt).1);
    }

    #[test]
    fn uv_is_the_test_runner_in_a_uv_project_and_an_alternative_for_ruff() {
        let tools = pack_tools(
            "python",
            Tree {
                pytest_suite: true,
                uv_project: true,
                ..Tree::default()
            },
        );
        let uv = tools.iter().find(|tool| tool.name == "uv").unwrap();
        let ruff = tools.iter().find(|tool| tool.name == "ruff").unwrap();
        assert!(matches!(&uv.probe, Probe::Bin("uv")));
        assert!(
            matches!(&ruff.probe, Probe::Any(probes) if probes.iter().any(|p| matches!(p, Probe::Bin("uv"))))
        );
        let plain = pack_tools(
            "python",
            Tree {
                pytest_suite: true,
                ..Tree::default()
            },
        );
        assert!(matches!(
            &plain
                .iter()
                .find(|tool| tool.name == "pytest")
                .unwrap()
                .probe,
            Probe::Run(_)
        ));
    }

    #[test]
    fn packs_without_external_tools_say_so() {
        let (text, ok) = render(&["web"], Tree::default(), &|_| false, PackageManager::Apt);
        assert!(ok);
        assert_eq!(text, "web pack\n  no external tools needed\n");
    }

    #[test]
    fn bin_probe_finds_an_executable_and_rejects_a_missing_name() {
        assert!(on_path("sh"));
        assert!(!on_path("sc-no-such-tool-0b7e"));
    }
}

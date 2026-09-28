// SPDX-License-Identifier: MPL-2.0
//! Compile, test, and lint commands for the non-Rust, non-Python packs.
//!
//! A missing compiler is reported and does not fail the process. A command
//! that runs is enforced.

use std::path::Path;
use std::process::Command;
use std::time::Instant;

use sc_core::{Finding, Gate, RunRecord};

use crate::command::{brief, run_cmd, CommandError};
use crate::pack::PackId;

pub struct ToolReport {
    pub findings: Vec<Finding>,
    pub runs: Vec<RunRecord>,
    pub ran: Vec<String>,
    pub skipped: Vec<String>,
    pub gates: Vec<Gate>,
}

struct Step {
    gate: &'static str,
    engine: &'static str,
    command: Option<String>,
    absent: String,
    /// A failing command with `enforce: false` is reported and does not fail the run.
    enforce: bool,
}

pub fn run(pack: PackId, root: &Path, deadline: Instant, user_lint: Option<&str>) -> ToolReport {
    let mut steps = plan(pack, root);
    if let Some(lint) = user_lint {
        steps.retain(|step| step.gate != "lint");
        steps.push(Step {
            gate: "lint",
            engine: "lint",
            command: Some(lint.to_string()),
            absent: "lint command is empty".into(),
            enforce: true,
        });
    }
    let mut report = ToolReport {
        findings: Vec::new(),
        runs: Vec::new(),
        ran: Vec::new(),
        skipped: Vec::new(),
        gates: Vec::new(),
    };
    for step in steps {
        apply(root, deadline, step, &mut report);
    }
    report
}

fn plan(pack: PackId, root: &Path) -> Vec<Step> {
    match pack {
        PackId::Node => node_plan(root),
        PackId::Bash => bash_plan(root),
        PackId::Go => go_plan(root),
        PackId::Java => java_plan(root),
        PackId::CSharp => csharp_plan(),
        PackId::Php => php_plan(root),
        PackId::Cpp => cpp_plan(root),
        PackId::Command | PackId::Rust | PackId::Python | PackId::Web => Vec::new(),
    }
}

fn node_plan(root: &Path) -> Vec<Step> {
    let files = list_files(root, &["js", "jsx", "mjs", "cjs", "ts", "tsx"]);
    let ts = files
        .iter()
        .any(|file| file.ends_with(".ts") || file.ends_with(".tsx"));
    let compile = if files.is_empty() {
        None
    } else if ts {
        let ts_files = filter_ext(&files, &[".ts", ".tsx"]);
        let js_files = filter_ext(&files, &[".js", ".mjs", ".cjs"]);
        via(
            "tsc",
            tsc_and_node(root.join("tsconfig.json").is_file(), &ts_files, &js_files),
        )
        .or_else(|| which("node").then(|| check_chain("node --check", &files)))
    } else if which("node") {
        Some(check_chain("node --check", &files))
    } else {
        via("node", check_chain("node --check", &files))
    };
    let test = npm_test(root).and_then(|command| {
        via(
            "c8",
            format!(
                "mkdir -p .sc/coverage && c8 --reporter=json --reports-dir=.sc/coverage {command}"
            ),
        )
        .or_else(|| via("npm", command))
    });
    vec![
        Step {
            gate: "types",
            engine: "compile",
            command: compile,
            absent: "node is not installed".into(),
            enforce: true,
        },
        Step {
            gate: "tests",
            engine: "tests",
            command: test,
            absent: "package.json has no test script".into(),
            enforce: true,
        },
        Step {
            gate: "lint",
            engine: "lint",
            command: if eslint_config(root) {
                via("eslint", "eslint .".into())
                    .or_else(|| which("npx").then(|| "npx --no-install eslint .".into()))
            } else {
                None
            },
            absent: "eslint is not configured".into(),
            enforce: true,
        },
    ]
}

fn bash_plan(root: &Path) -> Vec<Step> {
    let files = list_files(root, &["sh", "bash"]);
    vec![
        Step {
            gate: "types",
            engine: "compile",
            command: if files.is_empty() {
                None
            } else {
                via("bash", check_chain("bash -n", &files))
            },
            absent: "bash is not installed".into(),
            enforce: true,
        },
        Step {
            gate: "tests",
            engine: "tests",
            command: bash_tests(root),
            absent: "bats is not installed or no .bats suite exists".into(),
            enforce: true,
        },
        Step {
            gate: "lint",
            engine: "lint",
            command: if !files.is_empty() {
                via("shellcheck", format!("shellcheck {}", shell_join(&files)))
            } else {
                None
            },
            absent: "shellcheck is not installed".into(),
            enforce: true,
        },
    ]
}

pub fn go_cover_path(root: &Path) -> std::path::PathBuf {
    root.join(".sc").join("coverage").join("go.out")
}

fn go_plan(root: &Path) -> Vec<Step> {
    let _ = std::fs::remove_file(go_cover_path(root));
    vec![
        Step {
            gate: "types",
            engine: "compile",
            command: via("go", "go build ./...".into()),
            absent: "go is not installed".into(),
            enforce: true,
        },
        Step {
            gate: "tests",
            engine: "tests",
            command: via(
                "go",
                "mkdir -p .sc/coverage && go test -coverprofile=.sc/coverage/go.out ./...".into(),
            ),
            absent: "go is not installed".into(),
            enforce: true,
        },
        Step {
            gate: "lint",
            engine: "lint",
            command: via("go", "go vet ./...".into()),
            absent: "go is not installed".into(),
            enforce: true,
        },
    ]
}

fn java_plan(root: &Path) -> Vec<Step> {
    let files = list_files(root, &["java"]);
    let has_pom = root.join("pom.xml").is_file();
    let has_gradle = root.join("build.gradle").is_file() || root.join("build.gradle.kts").is_file();
    let javac_cmd = if files.is_empty() {
        None
    } else {
        via(
            "javac",
            format!(
                "javac -d /tmp/sc-javac-{} {}",
                std::process::id(),
                shell_join(&files)
            ),
        )
    };
    let compile = if has_pom {
        via("mvn", java_mvn("compile", true)).or(javac_cmd)
    } else if has_gradle {
        via("gradle", "gradle compileJava --quiet".into()).or(javac_cmd)
    } else {
        javac_cmd
    };
    let test = if has_pom {
        via("mvn", java_mvn("test", false))
    } else if has_gradle {
        let command = if gradle_uses_jacoco(root) {
            "gradle test jacocoTestReport --quiet"
        } else {
            "gradle test --quiet"
        };
        via("gradle", command.into())
    } else {
        None
    };
    let test_absent = if has_pom {
        "maven is not installed"
    } else if has_gradle {
        "gradle is not installed"
    } else {
        "no Maven or Gradle project detected"
    };
    vec![
        Step {
            gate: "types",
            engine: "compile",
            command: compile,
            absent: "javac, mvn, or gradle is not installed".into(),
            enforce: true,
        },
        Step {
            gate: "tests",
            engine: "tests",
            command: test,
            absent: test_absent.into(),
            enforce: true,
        },
        Step {
            gate: "lint",
            engine: "lint",
            command: None,
            absent: "no Java linter is configured".into(),
            enforce: true,
        },
    ]
}

fn csharp_plan() -> Vec<Step> {
    let prefix = "mkdir -p .sc/coverage .sc/nuget .sc/dotnet && DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1 DOTNET_SKIP_FIRST_TIME_EXPERIENCE=1 DOTNET_CLI_HOME=\"$PWD/.sc/dotnet\" NUGET_PACKAGES=\"$PWD/.sc/nuget\"";
    vec![
        Step {
            gate: "types",
            engine: "compile",
            command: via("dotnet", format!("{prefix} dotnet build --nologo -v q")),
            absent: "dotnet is not installed".into(),
            enforce: true,
        },
        Step {
            gate: "tests",
            engine: "tests",
            command: via("dotnet", format!("{prefix} {CSHARP_TEST}")),
            absent: "dotnet is not installed".into(),
            enforce: true,
        },
        Step {
            gate: "lint",
            engine: "lint",
            command: None,
            absent: "C# analyzers run as part of dotnet build".into(),
            enforce: true,
        },
    ]
}

fn php_plan(root: &Path) -> Vec<Step> {
    let files = list_files(root, &["php"]);
    vec![
        Step {
            gate: "types",
            engine: "compile",
            command: if !files.is_empty() {
                via("php", check_chain("php -l", &files))
            } else {
                None
            },
            absent: "php is not installed".into(),
            enforce: true,
        },
        Step {
            gate: "tests",
            engine: "tests",
            command: if root.join("vendor/bin/phpunit").is_file() {
                via("php", php_test("vendor/bin/phpunit"))
            } else if has_php_tests(root) {
                via("phpunit", php_test("phpunit"))
            } else {
                None
            },
            absent: "phpunit is not installed".into(),
            enforce: true,
        },
        Step {
            gate: "lint",
            engine: "lint",
            command: if root.join("phpstan.neon").is_file()
                || root.join("phpstan.neon.dist").is_file()
            {
                via("phpstan", "phpstan analyse --no-progress".into())
            } else if !files.is_empty() {
                via(
                    "phpstan",
                    format!(
                        "phpstan analyse --no-progress --level 0 {}",
                        shell_join(&files)
                    ),
                )
            } else {
                None
            },
            absent: "phpstan is not installed".into(),
            enforce: true,
        },
    ]
}

fn cpp_plan(root: &Path) -> Vec<Step> {
    let files = list_files(root, &["c", "cc", "cpp", "cxx"]);
    let (c_files, cxx_files) = split_c_family(&files);
    vec![
        Step {
            gate: "types",
            engine: "compile",
            command: cpp_compile(root, &c_files, &cxx_files),
            absent: cpp_absent(&c_files, &cxx_files),
            enforce: types_enforced(root),
        },
        Step {
            gate: "tests",
            engine: "tests",
            command: if cpp_has_ctest(root) {
                via("lcov", cpp_coverage())
            } else {
                None
            },
            absent: "CTest suite is not present".into(),
            enforce: true,
        },
        Step {
            gate: "lint",
            engine: "lint",
            command: if root.join("compile_commands.json").is_file() && !files.is_empty() {
                via("clang-tidy", format!("clang-tidy {}", shell_join(&files)))
            } else {
                None
            },
            absent: "clang-tidy is not installed".into(),
            enforce: true,
        },
    ]
}

#[inline(never)]
fn apply(root: &Path, deadline: Instant, step: Step, report: &mut ToolReport) {
    let Some(command) = step.command else {
        report.skipped.push(step.engine.into());
        report.findings.push(unavailable(step.engine, &step.absent));
        report.gates.push(reported(step.gate, &step.absent));
        return;
    };
    match shell(root, &command, deadline) {
        Ok(captured) => {
            report.runs.push(RunRecord {
                engine: step.engine.into(),
                command: command.clone(),
                exit_code: captured.status.code(),
                duration_ms: u64::try_from(captured.elapsed.as_millis()).unwrap_or(u64::MAX),
                budget_ms: None,
            });
            if captured.status.success() {
                report.ran.push(step.engine.into());
                report.gates.push(enforced(step.gate, true, ""));
            } else if tool_missing(&captured.stderr, &captured.stdout) {
                report.skipped.push(step.engine.into());
                report.findings.push(unavailable(step.engine, &step.absent));
                report.gates.push(reported(step.gate, &step.absent));
            } else {
                report.ran.push(step.engine.into());
                let detail = brief(&format!("{}\n{}", captured.stdout, captured.stderr));
                let failed = if detail.is_empty() {
                    format!("{} failed", step.engine)
                } else {
                    format!("{} failed: {detail}", step.engine)
                };
                if step.enforce {
                    let rule = match step.engine {
                        "tests" => "test.failed",
                        "lint" => "lint.failed",
                        _ => "compile.failed",
                    };
                    report.findings.push(Finding {
                        id: format!("{}:failed", step.engine),
                        rule: rule.into(),
                        engine: step.engine.into(),
                        severity: "error".into(),
                        file: ".".into(),
                        span: None,
                        symbol: None,
                        message: failed,
                        evidence: serde_json::json!({"command": command}),
                        suggested_action: Some("Fix the failure and re-run".into()),
                        disposition: String::new(),
                    });
                    report.gates.push(enforced(
                        step.gate,
                        false,
                        &format!("{} failed", step.engine),
                    ));
                } else {
                    let reason = format!(
                        "{failed}. Include paths and generated headers are not known without CMakeLists.txt or compile_commands.json, so this types check is not enforced."
                    );
                    report.findings.push(Finding {
                        id: format!("{}:failed", step.engine),
                        rule: "compile.failed".into(),
                        engine: step.engine.into(),
                        severity: "warning".into(),
                        file: ".".into(),
                        span: None,
                        symbol: None,
                        message: reason.clone(),
                        evidence: serde_json::json!({"command": command}),
                        suggested_action: Some(
                            "Fix the compiler error, or pass --pack if this tree is not C or C++."
                                .into(),
                        ),
                        disposition: String::new(),
                    });
                    report.gates.push(Gate {
                        id: step.gate.into(),
                        pass: false,
                        enforced: false,
                        reason: Some(reason),
                    });
                }
            }
        }
        Err(err) => {
            report.skipped.push(step.engine.into());
            let message = err.message(step.engine);
            let mut finding = unavailable(step.engine, &message);
            if matches!(err, CommandError::Timeout) {
                finding.suggested_action = Some(crate::command::TIMEOUT_FIX.into());
            }
            report.findings.push(finding);
            report.gates.push(reported(step.gate, &message));
        }
    }
}

fn bash_tests(root: &Path) -> Option<String> {
    let bats = list_files(root, &["bats"]);
    if bats.is_empty() {
        return None;
    }
    let libs: Vec<String> = list_files(root, &["sh"])
        .into_iter()
        .filter(|file| {
            file.starts_with("scripts/")
                || file.starts_with("bin/")
                || file.contains("/scripts/")
                || file.contains("/bin/")
        })
        .collect();
    let targets = shell_join(&bats);
    let mut script = String::from("mkdir -p .sc/coverage/kcov");
    for file in &libs {
        script.push_str(&format!(
            " && kcov --cobertura-only .sc/coverage/kcov {} a >/dev/null || true",
            shell_quote(file)
        ));
    }
    script.push_str(&format!(" && bats {targets}"));
    via("kcov", script).or_else(|| via("bats", format!("bats {targets}")))
}

fn java_mvn(goal: &str, skip_tests: bool) -> String {
    let skip = if skip_tests { "-DskipTests " } else { "" };
    if goal == "test" {
        format!(
            "mvn -q -Dmaven.repo.local=.sc/m2 {skip}-Dmaven.compiler.source=17 -Dmaven.compiler.target=17 org.jacoco:jacoco-maven-plugin:0.8.12:prepare-agent test; status=$?; if find . -name jacoco.exec -not -path './.sc/*' -print -quit | grep -q jacoco.exec; then mvn -q -Dmaven.repo.local=.sc/m2 org.jacoco:jacoco-maven-plugin:0.8.12:report || true; fi; exit $status"
        )
    } else {
        format!(
            "mvn -q -Dmaven.repo.local=.sc/m2 {skip}-Dmaven.compiler.source=17 -Dmaven.compiler.target=17 {goal}"
        )
    }
}

fn gradle_uses_jacoco(root: &Path) -> bool {
    ["build.gradle", "build.gradle.kts"].iter().any(|name| {
        std::fs::read_to_string(root.join(name))
            .map(|text| text.contains("jacoco"))
            .unwrap_or(false)
    })
}

const CSHARP_TEST: &str = "dotnet test --nologo -v q /p:CollectCoverage=true /p:IncludeTestAssembly=true /p:CoverletOutputFormat=cobertura /p:CoverletOutput=.sc/coverage/csharp.cobertura.xml; status=$?; exit $status";

fn php_test(bin: &str) -> String {
    let covered = if bin.contains('/') {
        format!("php -d pcov.enabled=1 {bin}")
    } else {
        format!("php -d pcov.enabled=1 \"$(command -v {bin})\"")
    };
    let plain = if bin.contains('/') {
        format!("php {bin}")
    } else {
        bin.to_string()
    };
    format!(
        "mkdir -p .sc/coverage && {covered} --coverage-clover .sc/coverage/clover.xml; status=$?; if [ \"$status\" -ne 0 ] && [ ! -f .sc/coverage/clover.xml ]; then {plain}; exit $?; fi; exit $status"
    )
}

fn cpp_types_absent(compiler_present: bool, sources_empty: bool) -> &'static str {
    if compiler_present && sources_empty {
        "no .c, .cc, .cpp, or .cxx file to compile"
    } else {
        "g++ or cmake is not installed"
    }
}

/// A syntax check cannot see include paths or generated headers unless the
/// project has `CMakeLists.txt` or `compile_commands.json`.
fn types_enforced(root: &Path) -> bool {
    root.join("CMakeLists.txt").is_file() || root.join("compile_commands.json").is_file()
}

fn split_c_family(files: &[String]) -> (Vec<String>, Vec<String>) {
    let mut c_files = Vec::new();
    let mut cxx_files = Vec::new();
    for file in files {
        if std::path::Path::new(file)
            .extension()
            .and_then(|ext| ext.to_str())
            == Some("c")
        {
            c_files.push(file.clone());
        } else {
            cxx_files.push(file.clone());
        }
    }
    (c_files, cxx_files)
}

fn cpp_absent(c_files: &[String], cxx_files: &[String]) -> String {
    if c_files.is_empty() && cxx_files.is_empty() {
        return cpp_types_absent(
            which("cc") || which("g++") || which("cmake") || image_present(),
            true,
        )
        .into();
    }
    let cc_missing = !c_files.is_empty() && !which("cc") && !image_present();
    let gxx_missing = !cxx_files.is_empty() && !which("g++") && !image_present();
    missing_compiler_message(cc_missing, gxx_missing).into()
}

fn missing_compiler_message(cc_missing: bool, gxx_missing: bool) -> &'static str {
    match (cc_missing, gxx_missing) {
        (true, true) => "cc or g++ is not installed",
        (true, false) => "cc is not installed",
        (false, true) => "g++ is not installed",
        (false, false) => "cmake is not installed",
    }
}

fn syntax_group(bin: &str, prefix: &str, files: &[String]) -> Option<String> {
    if files.is_empty() {
        Some(String::new())
    } else {
        via(bin, check_chain(prefix, files))
    }
}

fn cpp_compile(root: &Path, c_files: &[String], cxx_files: &[String]) -> Option<String> {
    if !c_files.is_empty() || !cxx_files.is_empty() {
        let c = syntax_group("cc", "cc -fsyntax-only -x c", c_files);
        let cxx = syntax_group("g++", "g++ -fsyntax-only", cxx_files);
        if let (Some(c), Some(cxx)) = (c, cxx) {
            return Some(match (c.is_empty(), cxx.is_empty()) {
                (false, true) => c,
                (true, false) => cxx,
                (false, false) => format!("{c} && {cxx}"),
                (true, true) => String::new(),
            });
        }
    }
    if root.join("CMakeLists.txt").is_file() {
        via(
            "cmake",
            "cmake -S . -B .sc/cmake-build && cmake --build .sc/cmake-build".into(),
        )
    } else {
        None
    }
}

fn cpp_has_ctest(root: &Path) -> bool {
    if root.join("CTestTestfile.cmake").is_file() {
        return true;
    }
    std::fs::read_to_string(root.join("CMakeLists.txt"))
        .map(|text| text.contains("enable_testing") || text.contains("add_test"))
        .unwrap_or(false)
}

fn cpp_coverage() -> String {
    "cmake -S . -B .sc/cmake-build -DCMAKE_BUILD_TYPE=Debug -DCMAKE_C_FLAGS=--coverage -DCMAKE_CXX_FLAGS=--coverage -DCMAKE_EXE_LINKER_FLAGS=--coverage && cmake --build .sc/cmake-build && ctest --test-dir .sc/cmake-build --output-on-failure; status=$?; mkdir -p .sc/coverage && lcov --capture --directory .sc/cmake-build --output-file .sc/coverage/cpp.info || true; exit $status".into()
}

fn has_php_tests(root: &Path) -> bool {
    root.join("phpunit.xml").is_file()
        || root.join("phpunit.xml.dist").is_file()
        || list_files(root, &["php"])
            .iter()
            .any(|file| file.contains("Test") || file.contains("/tests/"))
}

fn npm_test(root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(root.join("package.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let test = value.pointer("/scripts/test")?.as_str()?;
    if test.contains("no test specified") {
        return None;
    }
    Some("npm test --silent".into())
}

fn eslint_config(root: &Path) -> bool {
    [
        "eslint.config.js",
        "eslint.config.cjs",
        "eslint.config.mjs",
        ".eslintrc",
        ".eslintrc.js",
        ".eslintrc.json",
    ]
    .iter()
    .any(|name| root.join(name).is_file())
}

fn list_files(root: &Path, exts: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    walk(root, root, 0, exts, &mut out);
    out
}

fn walk(root: &Path, dir: &Path, depth: u32, exts: &[&str], out: &mut Vec<String>) {
    if depth > 5 || out.len() >= 200 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with('.') || name == "node_modules" || name == "target" || name == "dist" {
            continue;
        }
        if path.is_dir() {
            walk(root, &path, depth + 1, exts, out);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| exts.contains(&e))
        {
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
}

fn filter_ext(files: &[String], exts: &[&str]) -> Vec<String> {
    files
        .iter()
        .filter(|file| exts.iter().any(|ext| file.ends_with(ext)))
        .cloned()
        .collect()
}

/// `tsc --noEmit` with no files obeys tsconfig `include` and skips the rest.
/// Current `tsc` also refuses file arguments when `tsconfig.json` is present,
/// so that case extends the project config and lists every TypeScript file.
/// JavaScript is syntax-checked separately.
fn tsc_and_node(has_tsconfig: bool, ts: &[String], js: &[String]) -> String {
    let tsc = if has_tsconfig {
        let include = ts
            .iter()
            .map(|file| format!("\"../{}\"", json_escape(file)))
            .collect::<Vec<_>>()
            .join(",");
        let overlay = format!("{{\"extends\":\"../tsconfig.json\",\"include\":[{include}]}}");
        format!(
            "mkdir -p .sc && printf '%s\\n' {} > .sc/tsconfig.sc.json && tsc --noEmit -p .sc/tsconfig.sc.json",
            shell_quote(&overlay)
        )
    } else {
        format!("tsc --noEmit {}", shell_join(ts))
    };
    if js.is_empty() {
        tsc
    } else {
        let node = check_chain("node --check", js);
        format!(
            "{{ {tsc}; _sc_tsc=$?; {node}; _sc_node=$?; [ \"$_sc_tsc\" -eq 0 ] && [ \"$_sc_node\" -eq 0 ]; }}"
        )
    }
}

fn json_escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

fn check_chain(bin: &str, files: &[String]) -> String {
    files
        .iter()
        .map(|file| format!("{bin} {}", shell_quote(file)))
        .collect::<Vec<_>>()
        .join(" && ")
}

fn shell_join(files: &[String]) -> String {
    files
        .iter()
        .map(|file| shell_quote(file))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

const IMAGE: &str = "scorecard-tools:latest";

fn via(bin: &str, command: String) -> Option<String> {
    if which(bin) {
        Some(command)
    } else if image_present() {
        Some(docker_wrap(&command))
    } else {
        None
    }
}

fn image_present() -> bool {
    use std::sync::OnceLock;
    static PRESENT: OnceLock<bool> = OnceLock::new();
    *PRESENT.get_or_init(|| {
        socket_has_image("/var/run/docker.sock")
            || desktop_socket().is_some_and(|sock| socket_has_image(&sock))
    })
}

fn socket_has_image(sock: &str) -> bool {
    Command::new("docker")
        .env("DOCKER_HOST", format!("unix://{sock}"))
        .args(["image", "inspect", "--format", "{{.Id}}", IMAGE])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn desktop_socket() -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let path = std::path::PathBuf::from(home).join(".docker/desktop/docker.sock");
    path.exists().then(|| path.to_string_lossy().to_string())
}

fn docker_wrap(script: &str) -> String {
    format!(
        "docker run --rm --network host --user \"$(id -u):$(id -g)\" -e HOME=/tmp -e CARGO_HOME=\"$PWD/.sc/cargo\" -e UV_CACHE_DIR=\"$PWD/.sc/uv\" -e PIP_CACHE_DIR=\"$PWD/.sc/pip\" -e GOCACHE=\"$PWD/.sc/go/cache\" -e GOMODCACHE=\"$PWD/.sc/go/mod\" -e GOPATH=\"$PWD/.sc/go\" -e GOTOOLCHAIN=local -v \"$PWD\":\"$PWD\" -w \"$PWD\" {IMAGE} sh -c {}",
        shell_quote(script)
    )
}

/// Run `script` on the host when its first program exists. Otherwise run it
/// in the tools image, when that image is present.
pub fn prepare(script: &str) -> String {
    let bin = first_token(script);
    if bin.is_empty() || bin == "docker" || which(&bin) {
        script.to_string()
    } else if image_present() {
        docker_wrap(script)
    } else {
        script.to_string()
    }
}

pub fn force_image(script: &str) -> String {
    if image_present() {
        docker_wrap(script)
    } else {
        script.to_string()
    }
}

pub fn host_has(name: &str) -> bool {
    which(name)
}

pub fn run_script(
    root: &Path,
    script: &str,
    deadline: Instant,
) -> Result<crate::command::Captured, crate::command::CommandError> {
    shell(root, &prepare(script), deadline)
}

pub fn apply_docker_host(cmd: &mut Command) {
    docker_host(cmd);
}

fn first_token(script: &str) -> String {
    script
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches('\'')
        .to_string()
}

fn docker_host(cmd: &mut Command) {
    if std::env::var_os("DOCKER_HOST").is_some() {
        return;
    }
    if socket_has_image("/var/run/docker.sock") {
        cmd.env("DOCKER_HOST", "unix:///var/run/docker.sock");
        return;
    }
    if let Some(sock) = desktop_socket() {
        cmd.env("DOCKER_HOST", format!("unix://{sock}"));
    }
}

fn which(name: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {name} >/dev/null")])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn tool_missing(stderr: &str, stdout: &str) -> bool {
    let text = format!("{stderr}\n{stdout}").to_ascii_lowercase();
    text.contains("command not found")
        || text.contains("no such command")
        || text.contains("not recognized")
        || text.contains("unable to locate a java runtime")
        || text.contains("no java runtime present")
}

fn shell(
    root: &Path,
    script: &str,
    deadline: Instant,
) -> Result<crate::command::Captured, CommandError> {
    let timeout = crate::command::budget_left(deadline)?;
    let mut cmd = Command::new("sh");
    docker_host(&mut cmd);
    cmd.current_dir(root)
        .arg("-c")
        .arg(script)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    run_cmd(&mut cmd, timeout)
}

fn enforced(id: &str, pass: bool, reason: &str) -> Gate {
    Gate {
        id: id.into(),
        pass,
        enforced: true,
        reason: if pass || reason.is_empty() {
            None
        } else {
            Some(reason.into())
        },
    }
}

fn reported(id: &str, reason: &str) -> Gate {
    Gate {
        id: id.into(),
        pass: false,
        enforced: false,
        reason: Some(reason.into()),
    }
}

fn unavailable(engine: &str, message: &str) -> Finding {
    Finding {
        id: format!("{engine}:unavailable"),
        rule: "engine.unavailable".into(),
        engine: engine.into(),
        severity: "warning".into(),
        file: ".".into(),
        span: None,
        symbol: None,
        message: message.into(),
        evidence: serde_json::json!({}),
        suggested_action: Some("Install the tool and re-run".into()),
        disposition: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn go_plan_builds_tests_and_vets_when_go_exists() {
        if !which("go") {
            return;
        }
        let steps = go_plan(std::path::Path::new("."));
        assert!(steps.iter().any(|step| {
            step.command.as_deref().is_some_and(|command| {
                command.contains("go test -coverprofile=.sc/coverage/go.out")
            })
        }));
        assert!(steps
            .iter()
            .any(|step| step.command.as_deref() == Some("go vet ./...")));
    }

    #[test]
    fn tsc_names_typescript_files_and_node_checks_javascript() {
        let cmd = tsc_and_node(
            true,
            &["src/ok.ts".into(), "src/bad.ts".into()],
            &["src/bad.js".into()],
        );
        assert!(
            cmd.contains("tsc --noEmit -p .sc/tsconfig.sc.json"),
            "{cmd}"
        );
        assert!(cmd.contains("../src/bad.ts"), "{cmd}");
        assert!(cmd.contains("../src/ok.ts"), "{cmd}");
        assert!(cmd.contains("node --check 'src/bad.js'"), "{cmd}");
        assert!(!cmd.contains("tsc --noEmit 'src/bad.ts'"), "{cmd}");
        let bare = tsc_and_node(false, &["src/bad.ts".into()], &[]);
        assert_eq!(bare, "tsc --noEmit 'src/bad.ts'");
    }

    #[test]
    fn java_plan_runs_jacoco_when_a_pom_exists() {
        let root = std::env::temp_dir().join(format!("sc-java-plan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("pom.xml"), "<project></project>").unwrap();
        let steps = java_plan(&root);
        let test = steps.iter().find(|step| step.gate == "tests").unwrap();
        let command = test.command.clone().unwrap_or_default();
        assert!(command.contains("jacoco"), "{command}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn java_plan_runs_gradle_tests_without_jacoco_text() {
        if !which("gradle") && !image_present() {
            return;
        }
        let root = std::env::temp_dir().join(format!("sc-java-gradle-plan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("build.gradle"), "plugins { id 'java' }\n").unwrap();
        let steps = java_plan(&root);
        let test = steps.iter().find(|step| step.gate == "tests").unwrap();
        let command = test.command.clone().unwrap_or_default();
        assert!(command.contains("gradle test --quiet"), "{command}");
        assert!(!command.contains("jacocoTestReport"), "{command}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn macos_java_stub_is_a_missing_tool() {
        let stderr = "The operation couldn’t be completed. Unable to locate a Java Runtime.\nPlease visit http://www.java.com for information on installing Java.\n";
        assert!(tool_missing(stderr, ""));
        assert!(!tool_missing("error: cannot find symbol", ""));
    }

    #[test]
    fn gxx_present_with_no_translation_unit_is_not_a_missing_compiler() {
        assert_eq!(
            cpp_types_absent(true, true),
            "no .c, .cc, .cpp, or .cxx file to compile"
        );
        assert_eq!(
            cpp_types_absent(false, true),
            "g++ or cmake is not installed"
        );
        assert_eq!(
            cpp_types_absent(true, false),
            "g++ or cmake is not installed"
        );
        assert_eq!(missing_compiler_message(true, false), "cc is not installed");
        assert_eq!(
            missing_compiler_message(false, true),
            "g++ is not installed"
        );
        assert_eq!(
            missing_compiler_message(true, true),
            "cc or g++ is not installed"
        );
    }

    #[test]
    fn valid_c_with_a_makefile_uses_cc_and_does_not_fail_types() {
        let root = std::env::temp_dir().join(format!("sc-valid-c-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("Makefile"), "all:\n").unwrap();
        std::fs::write(
            root.join("demo.c"),
            "#include <stdlib.h>\nint main(void) {\n    int *p = malloc(sizeof *p);\n    int new = 0;\n    free(p);\n    return new;\n}\n",
        )
        .unwrap();
        let steps = cpp_plan(&root);
        let types = steps.iter().find(|step| step.gate == "types").unwrap();
        let command = types.command.clone().unwrap_or_default();
        assert!(command.contains("cc -fsyntax-only -x c"), "{command}");
        assert!(!command.contains("g++"), "{command}");
        assert!(!types.enforce);
        if which("cc") {
            let report = run(
                PackId::Cpp,
                &root,
                Instant::now() + std::time::Duration::from_secs(30),
                None,
            );
            let gate = report.gates.iter().find(|gate| gate.id == "types").unwrap();
            assert!(gate.pass, "{gate:?} {:?}", report.findings);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cmake_and_makefile_keeps_a_compile_error_enforced() {
        if !which("cc") {
            return;
        }
        let root = std::env::temp_dir().join(format!("sc-cmake-make-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("CMakeLists.txt"),
            "cmake_minimum_required(VERSION 3.16)\n",
        )
        .unwrap();
        std::fs::write(root.join("Makefile"), "all:\n").unwrap();
        std::fs::write(root.join("demo.c"), "int main(void) { return }\n").unwrap();
        let steps = cpp_plan(&root);
        let types = steps.iter().find(|step| step.gate == "types").unwrap();
        assert!(types.enforce);
        let report = run(
            PackId::Cpp,
            &root,
            Instant::now() + std::time::Duration::from_secs(30),
            None,
        );
        let gate = report.gates.iter().find(|gate| gate.id == "types").unwrap();
        assert!(!gate.pass, "{gate:?} {:?}", report.findings);
        assert!(gate.enforced, "{gate:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_header_on_a_makefile_is_not_enforced() {
        if !which("cc") {
            return;
        }
        let root = std::env::temp_dir().join(format!("sc-cc-miss-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("Makefile"), "all:\n").unwrap();
        std::fs::write(
            root.join("demo.c"),
            "#include \"missing.h\"\nint main(void) { return 0; }\n",
        )
        .unwrap();
        let report = run(
            PackId::Cpp,
            &root,
            Instant::now() + std::time::Duration::from_secs(30),
            None,
        );
        let gate = report.gates.iter().find(|gate| gate.id == "types").unwrap();
        assert!(!gate.pass, "{gate:?}");
        assert!(!gate.enforced, "{gate:?}");
        let reason = gate.reason.clone().unwrap_or_default();
        assert!(reason.contains("not enforced"), "{reason}");
        assert!(reason.contains("Include paths"), "{reason}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cxx_file_uses_gxx() {
        let root = std::env::temp_dir().join(format!("sc-cxx-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("demo.cpp"), "int main() { return 0; }\n").unwrap();
        let steps = cpp_plan(&root);
        let types = steps.iter().find(|step| step.gate == "types").unwrap();
        let command = types.command.clone().unwrap_or_default();
        assert!(command.contains("g++ -fsyntax-only"), "{command}");
        assert!(!command.contains("cc -fsyntax-only"), "{command}");
        assert!(!types.enforce);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn c_header_not_found_is_not_a_missing_tool() {
        // A compiler error about a missing header is a real failure,
        // not evidence that the compiler is missing.
        let stderr = "projects/OS400/curlcl.c:31:10: fatal error: 'milib.h' file not found\n";
        assert!(!tool_missing(stderr, ""));
    }

    #[test]
    fn config_not_found_is_not_a_missing_tool() {
        // A test that outputs "config file not found" is a real test
        // failure, not evidence that the test runner is missing.
        let stdout = "config file not found\n";
        assert!(!tool_missing("", stdout));
    }

    #[test]
    fn command_not_found_still_detected() {
        // The shell's own "command not found" must still be caught.
        assert!(tool_missing("sh: g++: command not found", ""));
        assert!(tool_missing("bash: npm: command not found", ""));
        assert!(tool_missing("g++: command not found", ""));
    }

    #[test]
    fn no_such_command_still_detected() {
        assert!(tool_missing("no such command: faketool", ""));
    }

    #[test]
    fn prepare_keeps_a_host_program() {
        let script = prepare("sh -c true");
        assert_eq!(script, "sh -c true");
    }

    #[test]
    fn prepare_uses_the_image_when_the_program_is_missing() {
        let script = prepare("sc-missing-bin-zz --version");
        if image_present() {
            assert!(script.contains("scorecard-tools:latest"), "{script}");
            assert!(script.contains("CARGO_HOME="), "{script}");
            assert!(script.contains("-v \"$PWD\":\"$PWD\""), "{script}");
        } else {
            assert_eq!(script, "sc-missing-bin-zz --version");
        }
    }

    #[test]
    fn apply_records_success_failure_and_a_missing_tool() {
        let root = std::env::temp_dir().join(format!("sc-apply-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        let mut report = ToolReport {
            findings: Vec::new(),
            runs: Vec::new(),
            ran: Vec::new(),
            skipped: Vec::new(),
            gates: Vec::new(),
        };
        apply(
            &root,
            deadline,
            Step {
                gate: "types",
                engine: "compile",
                command: Some("true".into()),
                absent: "missing".into(),
                enforce: true,
            },
            &mut report,
        );
        apply(
            &root,
            deadline,
            Step {
                gate: "tests",
                engine: "tests",
                command: Some("false".into()),
                absent: "missing".into(),
                enforce: true,
            },
            &mut report,
        );
        apply(
            &root,
            deadline,
            Step {
                gate: "lint",
                engine: "lint",
                command: Some("sc-not-a-tool-zz".into()),
                absent: "lint is not installed".into(),
                enforce: true,
            },
            &mut report,
        );
        apply(
            &root,
            deadline,
            Step {
                gate: "types",
                engine: "compile",
                command: None,
                absent: "nothing to compile".into(),
                enforce: true,
            },
            &mut report,
        );
        assert!(report.ran.iter().any(|engine| engine == "compile"));
        assert!(report
            .findings
            .iter()
            .any(|finding| finding.rule == "test.failed"));
        assert!(report
            .skipped
            .iter()
            .any(|engine| engine == "lint" || engine == "compile"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn eslint_config_accepts_cjs_config_file() {
        let root = std::env::temp_dir().join(format!("sc-eslint-cjs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("eslint.config.cjs"), "module.exports = [];\n").unwrap();
        assert!(eslint_config(&root));
        let _ = std::fs::remove_dir_all(&root);
    }
}

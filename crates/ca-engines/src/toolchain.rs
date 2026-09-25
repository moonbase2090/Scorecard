//! Compile, test, and lint commands for the non-Rust, non-Python packs.
//!
//! A missing compiler is reported and does not fail the process. A command
//! that runs is enforced.

use std::path::Path;
use std::process::Command;
use std::time::Instant;

use ca_core::{Finding, Gate, RunRecord};

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
        PackId::Command | PackId::Rust | PackId::Python => Vec::new(),
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
        via("tsc", "tsc --noEmit".into())
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
                "mkdir -p .ca/coverage && c8 --reporter=json --reports-dir=.ca/coverage {command}"
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
        },
        Step {
            gate: "tests",
            engine: "tests",
            command: test,
            absent: "package.json has no test script".into(),
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
        },
        Step {
            gate: "tests",
            engine: "tests",
            command: bash_tests(root),
            absent: "bats is not installed or no .bats suite exists".into(),
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
        },
    ]
}

pub fn go_cover_path(root: &Path) -> std::path::PathBuf {
    root.join(".ca").join("coverage").join("go.out")
}

fn go_plan(root: &Path) -> Vec<Step> {
    let _ = std::fs::remove_file(go_cover_path(root));
    vec![
        Step {
            gate: "types",
            engine: "compile",
            command: via("go", "go build ./...".into()),
            absent: "go is not installed".into(),
        },
        Step {
            gate: "tests",
            engine: "tests",
            command: via(
                "go",
                "mkdir -p .ca/coverage && go test -coverprofile=.ca/coverage/go.out ./...".into(),
            ),
            absent: "go is not installed".into(),
        },
        Step {
            gate: "lint",
            engine: "lint",
            command: via("go", "go vet ./...".into()),
            absent: "go is not installed".into(),
        },
    ]
}

fn java_plan(root: &Path) -> Vec<Step> {
    let files = list_files(root, &["java"]);
    let javac_cmd = if files.is_empty() {
        None
    } else {
        via(
            "javac",
            format!(
                "javac -d /tmp/ca-javac-{} {}",
                std::process::id(),
                shell_join(&files)
            ),
        )
    };
    let compile = if root.join("pom.xml").is_file() {
        via("mvn", java_mvn("compile", true)).or(javac_cmd)
    } else if root.join("build.gradle").is_file() || root.join("build.gradle.kts").is_file() {
        via("gradle", "gradle compileJava --quiet".into()).or(javac_cmd)
    } else {
        javac_cmd
    };
    let test = if root.join("pom.xml").is_file() {
        via("mvn", java_mvn("test", false))
    } else if gradle_uses_jacoco(root) {
        via("gradle", "gradle test jacocoTestReport --quiet".into())
    } else {
        None
    };
    vec![
        Step {
            gate: "types",
            engine: "compile",
            command: compile,
            absent: "javac, mvn, or gradle is not installed".into(),
        },
        Step {
            gate: "tests",
            engine: "tests",
            command: test,
            absent: "maven is not installed".into(),
        },
        Step {
            gate: "lint",
            engine: "lint",
            command: None,
            absent: "no Java linter is configured".into(),
        },
    ]
}

fn csharp_plan() -> Vec<Step> {
    let prefix = "mkdir -p .ca/coverage .ca/nuget .ca/dotnet && DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1 DOTNET_SKIP_FIRST_TIME_EXPERIENCE=1 DOTNET_CLI_HOME=\"$PWD/.ca/dotnet\" NUGET_PACKAGES=\"$PWD/.ca/nuget\"";
    vec![
        Step {
            gate: "types",
            engine: "compile",
            command: via("dotnet", format!("{prefix} dotnet build --nologo -v q")),
            absent: "dotnet is not installed".into(),
        },
        Step {
            gate: "tests",
            engine: "tests",
            command: via("dotnet", format!("{prefix} {CSHARP_TEST}")),
            absent: "dotnet is not installed".into(),
        },
        Step {
            gate: "lint",
            engine: "lint",
            command: None,
            absent: "C# analyzers run as part of dotnet build".into(),
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
        },
    ]
}

fn cpp_plan(root: &Path) -> Vec<Step> {
    let files = list_files(root, &["c", "cc", "cpp", "cxx"]);
    vec![
        Step {
            gate: "types",
            engine: "compile",
            command: cpp_compile(root, &files),
            absent: "g++ or cmake is not installed".into(),
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
        },
    ]
}

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
                    message: if detail.is_empty() {
                        format!("{} failed", step.engine)
                    } else {
                        format!("{} failed: {detail}", step.engine)
                    },
                    evidence: serde_json::json!({"command": command}),
                    suggested_action: Some("Fix the failure and re-run".into()),
                    disposition: String::new(),
                });
                report.gates.push(enforced(
                    step.gate,
                    false,
                    &format!("{} failed", step.engine),
                ));
            }
        }
        Err(err) => {
            report.skipped.push(step.engine.into());
            report
                .findings
                .push(unavailable(step.engine, &err.message(step.engine)));
            report
                .gates
                .push(reported(step.gate, &err.message(step.engine)));
        }
    }
}

fn bash_tests(root: &Path) -> Option<String> {
    let bats = list_files(root, &["bats"]);
    if bats.is_empty() {
        return None;
    }
    let targets = shell_join(&bats);
    let script = format!(
        "mkdir -p .ca/coverage/kcov && kcov --clean --include-pattern=.sh --bash-parse-files-in-dir=. --cobertura-only .ca/coverage/kcov bats {targets}; status=$?; if [ \"$status\" -ne 0 ]; then bats {targets}; exit $?; fi; exit 0"
    );
    via("kcov", script).or_else(|| via("bats", format!("bats {targets}")))
}

fn java_mvn(goal: &str, skip_tests: bool) -> String {
    let skip = if skip_tests { "-DskipTests " } else { "" };
    if goal == "test" {
        format!(
            "mvn -q -Dmaven.repo.local=.ca/m2 {skip}-Dmaven.compiler.source=17 -Dmaven.compiler.target=17 org.jacoco:jacoco-maven-plugin:0.8.12:prepare-agent test; status=$?; if find . -name jacoco.exec -not -path './.ca/*' -print -quit | grep -q jacoco.exec; then mvn -q -Dmaven.repo.local=.ca/m2 org.jacoco:jacoco-maven-plugin:0.8.12:report || true; fi; exit $status"
        )
    } else {
        format!(
            "mvn -q -Dmaven.repo.local=.ca/m2 {skip}-Dmaven.compiler.source=17 -Dmaven.compiler.target=17 {goal}"
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

const CSHARP_TEST: &str = "dotnet test --nologo -v q; status=$?; if [ \"$status\" -eq 0 ] && command -v dotnet-coverage >/dev/null; then dotnet-coverage collect -f cobertura -o .ca/coverage/csharp.cobertura.xml -- dotnet test --nologo -v q || true; fi; exit $status";

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
        "mkdir -p .ca/coverage && {covered} --coverage-clover .ca/coverage/clover.xml; status=$?; if [ \"$status\" -ne 0 ] && [ ! -f .ca/coverage/clover.xml ]; then {plain}; exit $?; fi; exit $status"
    )
}

fn cpp_compile(root: &Path, files: &[String]) -> Option<String> {
    if !files.is_empty() {
        if let Some(command) = via("g++", check_chain("g++ -fsyntax-only", files)) {
            return Some(command);
        }
    }
    if root.join("CMakeLists.txt").is_file() {
        via(
            "cmake",
            "cmake -S . -B .ca/cmake-build && cmake --build .ca/cmake-build".into(),
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
    "cmake -S . -B .ca/cmake-build -DCMAKE_BUILD_TYPE=Debug -DCMAKE_C_FLAGS=--coverage -DCMAKE_CXX_FLAGS=--coverage -DCMAKE_EXE_LINKER_FLAGS=--coverage && cmake --build .ca/cmake-build && ctest --test-dir .ca/cmake-build --output-on-failure; status=$?; mkdir -p .ca/coverage && lcov --capture --directory .ca/cmake-build --output-file .ca/coverage/cpp.info || true; exit $status".into()
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
        "docker run --rm --network host --user \"$(id -u):$(id -g)\" -e HOME=/tmp -e CARGO_HOME=\"$PWD/.ca/cargo\" -e UV_CACHE_DIR=\"$PWD/.ca/uv\" -e PIP_CACHE_DIR=\"$PWD/.ca/pip\" -e GOCACHE=\"$PWD/.ca/go/cache\" -e GOMODCACHE=\"$PWD/.ca/go/mod\" -e GOPATH=\"$PWD/.ca/go\" -e GOTOOLCHAIN=local -v \"$PWD\":\"$PWD\" -w \"$PWD\" {IMAGE} sh -c {}",
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
    text.contains("not found")
        || text.contains("no such command")
        || text.contains("not recognized")
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
                command.contains("go test -coverprofile=.ca/coverage/go.out")
            })
        }));
        assert!(steps
            .iter()
            .any(|step| step.command.as_deref() == Some("go vet ./...")));
    }

    #[test]
    fn java_plan_runs_jacoco_when_a_pom_exists() {
        let root = std::env::temp_dir().join(format!("ca-java-plan-{}", std::process::id()));
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
    fn prepare_keeps_a_host_program() {
        let script = prepare("sh -c true");
        assert_eq!(script, "sh -c true");
    }

    #[test]
    fn prepare_uses_the_image_when_the_program_is_missing() {
        let script = prepare("ca-missing-bin-zz --version");
        if image_present() {
            assert!(script.contains("scorecard-tools:latest"), "{script}");
            assert!(script.contains("CARGO_HOME="), "{script}");
            assert!(script.contains("-v \"$PWD\":\"$PWD\""), "{script}");
        } else {
            assert_eq!(script, "ca-missing-bin-zz --version");
        }
    }
}

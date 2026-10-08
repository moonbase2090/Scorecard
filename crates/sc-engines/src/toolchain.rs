// SPDX-License-Identifier: MPL-2.0
//! Compile, test, and lint commands for the non-Rust, non-Python packs.
//!
//! A missing compiler is reported and does not fail the process. A command
//! that runs is enforced.

mod makefile;

#[cfg(test)]
use makefile::makefile_text_uncertain;
pub(crate) use makefile::MakefileFlags;

use crate::toolchain_plans;
use crate::toolchain_plans::include_only_failure;

#[cfg(test)]
use crate::toolchain_plans::{
    cpp_plan, cpp_types_absent, eslint_config, go_plan, java_plan, missing_compiler_message,
    node_coverage_command, tsc_and_node,
};

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

pub(super) struct Step {
    pub(super) gate: &'static str,
    pub(super) engine: &'static str,
    pub(super) command: Option<String>,
    pub(super) absent: String,
    /// A failing command with `enforce: false` is reported and does not fail the run
    /// when every error is a missing header. Any other error still fails the run.
    pub(super) enforce: bool,
    /// A failed command is reported and does not fail the run, whatever the error.
    pub(super) advisory_failure: bool,
}

pub fn run_with_generated(
    pack: PackId,
    root: &Path,
    deadline: Instant,
    user_lint: Option<&str>,
    coverage: bool,
    exclude: &[String],
    include_generated: &[String],
) -> ToolReport {
    let mut steps = plan_with_generated(pack, root, coverage, exclude, include_generated);
    if let Some(lint) = user_lint {
        steps.retain(|step| step.gate != "lint");
        steps.push(Step {
            gate: "lint",
            engine: "lint",
            command: Some(lint.to_string()),
            absent: "lint command is empty".into(),
            enforce: true,
            advisory_failure: false,
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

#[cfg(test)]
pub fn run(
    pack: PackId,
    root: &Path,
    deadline: Instant,
    user_lint: Option<&str>,
    coverage: bool,
) -> ToolReport {
    run_with_generated(pack, root, deadline, user_lint, coverage, &[], &[])
}

fn plan_with_generated(
    pack: PackId,
    root: &Path,
    coverage: bool,
    exclude: &[String],
    include_generated: &[String],
) -> Vec<Step> {
    toolchain_plans::plan_with_generated(pack, root, coverage, exclude, include_generated)
}

#[cfg(test)]
fn plan(pack: PackId, root: &Path, coverage: bool) -> Vec<Step> {
    plan_with_generated(pack, root, coverage, &[], &[])
}

pub fn go_cover_path(root: &Path) -> std::path::PathBuf {
    root.join(".sc").join("coverage").join("go.out")
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
                // Without a build database, only missing headers stay advisory.
                // An undeclared name or a syntax error still fails the gate.
                // A Makefile stays advisory unless it is plainly understood:
                // global flags, recipes that use only the known compiler
                // variables, and no include, shell, eval, file, or nested make.
                let output = format!("{}\n{}", captured.stdout, captured.stderr);
                let advisory =
                    step.advisory_failure || (!step.enforce && include_only_failure(&output));
                if !advisory {
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
                    let reason = if step.advisory_failure {
                        format!(
                            "{failed}. The types check does not see every flag this Makefile uses, so it is not enforced. Put -I and -D in a global CFLAGS, CPPFLAGS, or CXXFLAGS assignment."
                        )
                    } else {
                        format!(
                            "{failed}. Include paths and generated headers are not known without CMakeLists.txt or compile_commands.json, so this types check is not enforced."
                        )
                    };
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
                            if step.advisory_failure {
                                "Put -I and -D in a global CFLAGS, CPPFLAGS, or CXXFLAGS assignment. Recipes may use only CC, CXX, CFLAGS, CPPFLAGS, CXXFLAGS, LDFLAGS, LDLIBS, and TARGET_ARCH. Do not include a file, use $(shell), $(eval), $(file), or !=, recurse with make, or set a target-specific variable."
                                    .into()
                            } else {
                                "Fix the compiler error, or pass --pack if this tree is not C or C++."
                                    .into()
                            },
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

pub(crate) fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

const IMAGE: &str = "scorecard-tools:latest";

pub(crate) fn image_present() -> bool {
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

pub(crate) fn docker_wrap(script: &str) -> String {
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

pub(crate) fn which(name: &str) -> bool {
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
                true,
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
            true,
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
            true,
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
    fn include_errors_are_the_only_advisory_compile_failures() {
        assert!(include_only_failure(
            "demo.c:1:10: fatal error: 'missing.h' file not found\n"
        ));
        assert!(include_only_failure(
            "demo.c:1:10: fatal error: missing.h: No such file or directory\n"
        ));
        assert!(!include_only_failure(
            "bad.c:1:25: error: use of undeclared identifier 'x'\n"
        ));
        assert!(!include_only_failure(
            "demo.c:1:20: error: expected ';' after return statement\n"
        ));
        assert!(!include_only_failure(
            "demo.c:1:10: fatal error: 'missing.h' file not found\nbad.c:1:25: error: use of undeclared identifier 'x'\n"
        ));
        assert!(!include_only_failure("cc failed\n"));
        assert!(!include_only_failure(
            "demo.c:1:10: error: expected \"FILENAME\" or <FILENAME>\n"
        ));
        assert!(!include_only_failure(
            "demo.c:1:2: error: #include expects \"FILENAME\" or <FILENAME>\n"
        ));
    }

    #[test]
    fn undeclared_identifier_on_a_makefile_fails_the_types_gate() {
        if !which("cc") {
            return;
        }
        let root = std::env::temp_dir().join(format!("sc-cc-undeclared-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("Makefile"), "all:\n\tcc -o demo demo.c\n").unwrap();
        std::fs::write(root.join("demo.c"), "int main(void) { return 0; }\n").unwrap();
        std::fs::write(root.join("bad.c"), "int main(void) { return x; }\n").unwrap();
        let report = run(
            PackId::Cpp,
            &root,
            Instant::now() + std::time::Duration::from_secs(30),
            None,
            true,
        );
        let gate = report.gates.iter().find(|gate| gate.id == "types").unwrap();
        assert!(!gate.pass, "{gate:?} {:?}", report.findings);
        assert!(gate.enforced, "{gate:?} {:?}", report.findings);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_header_does_not_hide_a_later_error() {
        if !which("cc") {
            return;
        }
        let root = std::env::temp_dir().join(format!("sc-cc-order-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("Makefile"), "all:\n").unwrap();
        std::fs::write(
            root.join("a_gen.c"),
            "#include \"config.h\"\nint main(void) { return 0; }\n",
        )
        .unwrap();
        std::fs::write(root.join("b_bad.c"), "int main(void) { return x; }\n").unwrap();
        let report = run(
            PackId::Cpp,
            &root,
            Instant::now() + std::time::Duration::from_secs(30),
            None,
            true,
        );
        let gate = report.gates.iter().find(|gate| gate.id == "types").unwrap();
        assert!(!gate.pass, "{gate:?} {:?}", report.findings);
        assert!(gate.enforced, "{gate:?} {:?}", report.findings);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn makefile_defines_are_passed_to_the_types_check() {
        if !which("cc") {
            return;
        }
        let root = std::env::temp_dir().join(format!("sc-cc-flags-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("include")).unwrap();
        std::fs::write(root.join("include/config.h"), "#define OK 1\n").unwrap();
        std::fs::write(
            root.join("Makefile"),
            "CFLAGS = -DVERSION=1 -Iinclude\nall:\n\tcc -o demo main.c\n",
        )
        .unwrap();
        std::fs::write(
            root.join("main.c"),
            "#include \"config.h\"\nint main(void) { return VERSION + OK; }\n",
        )
        .unwrap();
        let steps = cpp_plan(&root);
        let command = steps
            .iter()
            .find(|step| step.gate == "types")
            .unwrap()
            .command
            .clone()
            .unwrap_or_default();
        assert!(command.contains("-DVERSION=1"), "{command}");
        assert!(command.contains("-Iinclude"), "{command}");
        let report = run(
            PackId::Cpp,
            &root,
            Instant::now() + std::time::Duration::from_secs(30),
            None,
            true,
        );
        let gate = report.gates.iter().find(|gate| gate.id == "types").unwrap();
        assert!(gate.pass, "{gate:?} {:?}", report.findings);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn quoted_makefile_define_reaches_the_compiler() {
        if !which("cc") {
            return;
        }
        let root = std::env::temp_dir().join(format!("sc-cc-quoted-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("Makefile"),
            "CFLAGS = -DVERSION=\\\"1.0\\\"\nall:\n\tcc $(CFLAGS) -o demo main.c\n",
        )
        .unwrap();
        std::fs::write(
            root.join("main.c"),
            "#include <stdio.h>\nint main(void) { puts(VERSION); return 0; }\n",
        )
        .unwrap();
        let steps = cpp_plan(&root);
        let command = steps
            .iter()
            .find(|step| step.gate == "types")
            .unwrap()
            .command
            .clone()
            .unwrap_or_default();
        assert!(command.contains("-DVERSION=\"1.0\""), "{command}");
        assert!(!command.contains("\\1.0"), "{command}");
        let report = run(
            PackId::Cpp,
            &root,
            Instant::now() + std::time::Duration::from_secs(30),
            None,
            true,
        );
        let gate = report.gates.iter().find(|gate| gate.id == "types").unwrap();
        assert!(gate.pass, "{gate:?} {:?}", report.findings);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_bare_include_fails_the_types_gate() {
        if !which("cc") {
            return;
        }
        let root = std::env::temp_dir().join(format!("sc-cc-bare-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("Makefile"), "all:\n").unwrap();
        std::fs::write(
            root.join("demo.c"),
            "#include\nint main(void) { return 0; }\n",
        )
        .unwrap();
        let report = run(
            PackId::Cpp,
            &root,
            Instant::now() + std::time::Duration::from_secs(30),
            None,
            true,
        );
        let gate = report.gates.iter().find(|gate| gate.id == "types").unwrap();
        assert!(!gate.pass, "{gate:?} {:?}", report.findings);
        assert!(gate.enforced, "{gate:?} {:?}", report.findings);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn makefile_flags_follow_make_assignment() {
        fn flags(makefile: &str) -> (Vec<String>, Vec<String>) {
            let root = std::env::temp_dir().join(format!(
                "sc-make-flags-{}-{}",
                std::process::id(),
                makefile.len()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(root.join("Makefile"), makefile).unwrap();
            let got = MakefileFlags::read(&root);
            let _ = std::fs::remove_dir_all(&root);
            (got.c, got.cxx)
        }

        let (c, _) = flags("CFLAGS := -DVERSION=\\\"1.0\\\"\n");
        assert_eq!(c, vec!["-DVERSION=\"1.0\"".to_string()]);
        let (c, _) = flags("CFLAGS ?= -DVERSION=\\\"1.0\\\"\n");
        assert_eq!(c, vec!["-DVERSION=\"1.0\"".to_string()]);
        let (_, cxx) = flags("CXXFLAGS := -DVERSION=\\\"1.0\\\"\n");
        assert_eq!(cxx, vec!["-DVERSION=\"1.0\"".to_string()]);
        let (c, _) = flags("CFLAGS = -DHIDE\nCFLAGS =\n");
        assert!(c.is_empty(), "{c:?}");
        let (c, _) = flags("CFLAGS = -DKEEP\nCFLAGS ?= -DOTHER\n");
        assert_eq!(c, vec!["-DKEEP".to_string()]);
        let (c, _) = flags("CFLAGS =\nCFLAGS ?= -DHIDE\n");
        assert!(c.is_empty(), "{c:?}");
        let (c, _) = flags("CFLAGS = -DVERSION=\\\"1.0\\\"\nCFLAGS += -Iinclude\n");
        assert_eq!(
            c,
            vec!["-DVERSION=\"1.0\"".to_string(), "-Iinclude".to_string()]
        );
        let (c, _) = flags("CFLAGS = -DMSG=\\\"hello\\ world\\\"\n");
        assert_eq!(c, vec!["-DMSG=\"hello world\"".to_string()]);
        let (c, cxx) = flags("CPPFLAGS = -UHIDE\nCFLAGS = -DHIDE\nCXXFLAGS = -DKEEP\n");
        assert_eq!(c, vec!["-DHIDE".to_string(), "-UHIDE".to_string()]);
        assert_eq!(cxx, vec!["-DKEEP".to_string(), "-UHIDE".to_string()]);
    }

    #[test]
    fn colon_assign_makefile_define_reaches_the_compiler() {
        if !which("cc") {
            return;
        }
        let root = std::env::temp_dir().join(format!("sc-cc-colon-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("Makefile"),
            "CFLAGS := -DVERSION=\\\"1.0\\\"\nall:\n\tcc $(CFLAGS) -o demo main.c\n",
        )
        .unwrap();
        std::fs::write(
            root.join("main.c"),
            "#include <stdio.h>\nint main(void) { puts(VERSION); return 0; }\n",
        )
        .unwrap();
        let steps = cpp_plan(&root);
        let command = steps
            .iter()
            .find(|step| step.gate == "types")
            .unwrap()
            .command
            .clone()
            .unwrap_or_default();
        assert!(command.contains("-DVERSION=\"1.0\""), "{command}");
        let report = run(
            PackId::Cpp,
            &root,
            Instant::now() + std::time::Duration::from_secs(30),
            None,
            true,
        );
        let gate = report.gates.iter().find(|gate| gate.id == "types").unwrap();
        assert!(gate.pass, "{gate:?} {:?}", report.findings);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn conditional_makefile_define_reaches_the_compiler() {
        if !which("cc") {
            return;
        }
        let root = std::env::temp_dir().join(format!("sc-cc-qmark-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("Makefile"),
            "CFLAGS ?= -DVERSION=\\\"1.0\\\"\nall:\n\tcc $(CFLAGS) -o demo main.c\n",
        )
        .unwrap();
        std::fs::write(
            root.join("main.c"),
            "#include <stdio.h>\nint main(void) { puts(VERSION); return 0; }\n",
        )
        .unwrap();
        let steps = cpp_plan(&root);
        let command = steps
            .iter()
            .find(|step| step.gate == "types")
            .unwrap()
            .command
            .clone()
            .unwrap_or_default();
        assert!(command.contains("-DVERSION=\"1.0\""), "{command}");
        let report = run(
            PackId::Cpp,
            &root,
            Instant::now() + std::time::Duration::from_secs(30),
            None,
            true,
        );
        let gate = report.gates.iter().find(|gate| gate.id == "types").unwrap();
        assert!(gate.pass, "{gate:?} {:?}", report.findings);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_later_makefile_assignment_drops_the_macro() {
        if !which("cc") {
            return;
        }
        let root = std::env::temp_dir().join(format!("sc-cc-replace-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("Makefile"),
            "CFLAGS = -DHIDE\nCFLAGS =\nall:\n\tcc $(CFLAGS) -o demo main.c\n",
        )
        .unwrap();
        std::fs::write(
            root.join("main.c"),
            "int main(void) {\n#ifdef HIDE\n    return 0;\n#else\n    return x;\n#endif\n}\n",
        )
        .unwrap();
        let steps = cpp_plan(&root);
        let command = steps
            .iter()
            .find(|step| step.gate == "types")
            .unwrap()
            .command
            .clone()
            .unwrap_or_default();
        assert!(!command.contains("-DHIDE"), "{command}");
        let report = run(
            PackId::Cpp,
            &root,
            Instant::now() + std::time::Duration::from_secs(30),
            None,
            true,
        );
        let gate = report.gates.iter().find(|gate| gate.id == "types").unwrap();
        assert!(!gate.pass, "{gate:?} {:?}", report.findings);
        assert!(gate.enforced, "{gate:?} {:?}", report.findings);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn escaped_space_in_a_makefile_define_stays_one_flag() {
        if !which("cc") {
            return;
        }
        let root = std::env::temp_dir().join(format!("sc-cc-space-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("Makefile"),
            "CFLAGS = -DMSG=\\\"hello\\ world\\\"\nall:\n\tcc $(CFLAGS) -o demo main.c\n",
        )
        .unwrap();
        std::fs::write(
            root.join("main.c"),
            "#include <string.h>\nint main(void) { return strcmp(MSG, \"hello world\"); }\n",
        )
        .unwrap();
        let steps = cpp_plan(&root);
        let command = steps
            .iter()
            .find(|step| step.gate == "types")
            .unwrap()
            .command
            .clone()
            .unwrap_or_default();
        assert!(command.contains("-DMSG=\"hello world\""), "{command}");
        let report = run(
            PackId::Cpp,
            &root,
            Instant::now() + std::time::Duration::from_secs(30),
            None,
            true,
        );
        let gate = report.gates.iter().find(|gate| gate.id == "types").unwrap();
        assert!(gate.pass, "{gate:?} {:?}", report.findings);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn make_evaluates_a_variable_and_skips_an_untaken_branch() {
        if !which("make") {
            return;
        }
        fn flags(makefile: &str) -> Vec<String> {
            let root = std::env::temp_dir().join(format!(
                "sc-make-eval-{}-{}",
                std::process::id(),
                makefile.len()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(root.join("Makefile"), makefile).unwrap();
            let got = MakefileFlags::read(&root);
            let _ = std::fs::remove_dir_all(&root);
            got.c
        }
        let c = flags("DEFS = -DVERSION=\\\"1.0\\\"\nCFLAGS = -O2 $(DEFS)\nall:\n\texit 7\n");
        assert_eq!(c, vec!["-DVERSION=\"1.0\"".to_string()]);
        let c = flags("ifeq ($(NEVER),1)\nCFLAGS = -DHIDE\nendif\nall:\n\texit 7\n");
        assert!(c.is_empty(), "{c:?}");
        assert!(makefile_text_uncertain("CFLAGS = $(DEFS)\n"));
        assert!(makefile_text_uncertain(
            "ifeq ($(NEVER),1)\nCFLAGS = -DHIDE\nendif\n"
        ));
        assert!(!makefile_text_uncertain("CFLAGS = -DVERSION=1\n"));
    }

    #[test]
    fn a_variable_in_cflags_reaches_the_compiler() {
        if !which("cc") || !which("make") {
            return;
        }
        let root = std::env::temp_dir().join(format!("sc-cc-defs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("Makefile"),
            "DEFS = -DVERSION=\\\"1.0\\\"\nCFLAGS = -O2 $(DEFS)\nall:\n\tcc $(CFLAGS) -o demo main.c\n",
        )
        .unwrap();
        std::fs::write(
            root.join("main.c"),
            "#include <stdio.h>\nint main(void) { puts(VERSION); return 0; }\n",
        )
        .unwrap();
        let steps = cpp_plan(&root);
        let types = steps.iter().find(|step| step.gate == "types").unwrap();
        let command = types.command.clone().unwrap_or_default();
        assert!(command.contains("-DVERSION=\"1.0\""), "{command}");
        assert!(!types.advisory_failure);
        let report = run(
            PackId::Cpp,
            &root,
            Instant::now() + std::time::Duration::from_secs(30),
            None,
            true,
        );
        let gate = report.gates.iter().find(|gate| gate.id == "types").unwrap();
        assert!(gate.pass, "{gate:?} {:?}", report.findings);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_untaken_makefile_branch_is_not_passed_to_the_compiler() {
        if !which("cc") || !which("make") {
            return;
        }
        let root = std::env::temp_dir().join(format!("sc-cc-ifeq-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("Makefile"),
            "ifeq ($(NEVER),1)\nCFLAGS = -DHIDE\nendif\nall:\n\tcc $(CFLAGS) -o demo main.c\n",
        )
        .unwrap();
        std::fs::write(
            root.join("main.c"),
            "int main(void) {\n#ifdef HIDE\n    return 0;\n#else\n    return undefined_thing;\n#endif\n}\n",
        )
        .unwrap();
        let steps = cpp_plan(&root);
        let types = steps.iter().find(|step| step.gate == "types").unwrap();
        let command = types.command.clone().unwrap_or_default();
        assert!(!command.contains("-DHIDE"), "{command}");
        assert!(!types.advisory_failure);
        let report = run(
            PackId::Cpp,
            &root,
            Instant::now() + std::time::Duration::from_secs(30),
            None,
            true,
        );
        let gate = report.gates.iter().find(|gate| gate.id == "types").unwrap();
        assert!(!gate.pass, "{gate:?} {:?}", report.findings);
        assert!(gate.enforced, "{gate:?} {:?}", report.findings);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_later_undef_cancels_the_macro() {
        if !which("cc") || !which("make") {
            return;
        }
        let root = std::env::temp_dir().join(format!("sc-cc-undef-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("Makefile"),
            "CFLAGS = -DHIDE -UHIDE\nall:\n\tcc $(CFLAGS) -o demo main.c\n",
        )
        .unwrap();
        std::fs::write(
            root.join("main.c"),
            "int main(void) {\n#ifdef HIDE\n    return 0;\n#else\n    return undefined_thing;\n#endif\n}\n",
        )
        .unwrap();
        let built = std::process::Command::new("make")
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(!built.success());
        let steps = cpp_plan(&root);
        let types = steps.iter().find(|step| step.gate == "types").unwrap();
        let command = types.command.clone().unwrap_or_default();
        let define = command.find("-DHIDE").expect(&command);
        let undef = command.find("-UHIDE").expect(&command);
        assert!(define < undef, "{command}");
        assert!(!types.advisory_failure);
        let report = run(
            PackId::Cpp,
            &root,
            Instant::now() + std::time::Duration::from_secs(30),
            None,
            true,
        );
        let gate = report.gates.iter().find(|gate| gate.id == "types").unwrap();
        assert!(!gate.pass, "{gate:?} {:?}", report.findings);
        assert!(gate.enforced, "{gate:?} {:?}", report.findings);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn hide_unless_defined() -> &'static str {
        "int main(void) {\n#ifdef HIDE\n    return 0;\n#else\n    return undefined_thing;\n#endif\n}\n"
    }

    #[test]
    fn cppflags_undef_after_cflags_define_fails_the_gate() {
        if !which("cc") || !which("make") {
            return;
        }
        let root = std::env::temp_dir().join(format!("sc-cc-cpp-undef-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("Makefile"),
            "CPPFLAGS = -UHIDE\nCFLAGS = -DHIDE\nall:\n\t$(CC) $(CFLAGS) $(CPPFLAGS) -o demo main.c\n",
        )
        .unwrap();
        std::fs::write(root.join("main.c"), hide_unless_defined()).unwrap();
        let built = std::process::Command::new("make")
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(!built.success());
        let steps = cpp_plan(&root);
        let types = steps.iter().find(|step| step.gate == "types").unwrap();
        let command = types.command.clone().unwrap_or_default();
        let define = command.find("-DHIDE").expect(&command);
        let undef = command.find("-UHIDE").expect(&command);
        assert!(define < undef, "{command}");
        assert!(!types.advisory_failure);
        let report = run(
            PackId::Cpp,
            &root,
            Instant::now() + std::time::Duration::from_secs(30),
            None,
            true,
        );
        let gate = report.gates.iter().find(|gate| gate.id == "types").unwrap();
        assert!(!gate.pass, "{gate:?} {:?}", report.findings);
        assert!(gate.enforced, "{gate:?} {:?}", report.findings);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cflags_undef_before_cppflags_define_passes() {
        if !which("cc") || !which("make") {
            return;
        }
        let root = std::env::temp_dir().join(format!("sc-cc-cpp-def-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("Makefile"),
            "CPPFLAGS = -DHIDE\nCFLAGS = -UHIDE\nall:\n\t$(CC) $(CFLAGS) $(CPPFLAGS) -o demo main.c\n",
        )
        .unwrap();
        std::fs::write(root.join("main.c"), hide_unless_defined()).unwrap();
        let built = std::process::Command::new("make")
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(built.success());
        let steps = cpp_plan(&root);
        let types = steps.iter().find(|step| step.gate == "types").unwrap();
        let command = types.command.clone().unwrap_or_default();
        let undef = command.find("-UHIDE").expect(&command);
        let define = command.find("-DHIDE").expect(&command);
        assert!(undef < define, "{command}");
        assert!(!types.advisory_failure);
        let report = run(
            PackId::Cpp,
            &root,
            Instant::now() + std::time::Duration::from_secs(30),
            None,
            true,
        );
        let gate = report.gates.iter().find(|gate| gate.id == "types").unwrap();
        assert!(gate.pass, "{gate:?} {:?}", report.findings);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_hidden_makefile_flag_is_not_global() {
        fn read_flags(makefile: &str) -> MakefileFlags {
            let root = std::env::temp_dir().join(format!(
                "sc-make-shape-{}-{}",
                std::process::id(),
                makefile.len()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(root.join("Makefile"), makefile).unwrap();
            let got = MakefileFlags::read(&root);
            let _ = std::fs::remove_dir_all(&root);
            got
        }

        let recipe = read_flags("all:\n\tcc -DVERSION=\\\"1.0\\\" -o demo main.c\n");
        assert!(recipe.uncertain);
        assert!(recipe.c.is_empty(), "{:?}", recipe.c);
        let target = read_flags("demo: CFLAGS += -DVERSION=\\\"1.0\\\"\n");
        assert!(target.uncertain);
        assert!(target.c.is_empty(), "{:?}", target.c);
        let pattern = read_flags("%.o: CXXFLAGS += -Iinc\n");
        assert!(pattern.uncertain);
        assert!(pattern.cxx.is_empty(), "{:?}", pattern.cxx);
        let both = read_flags("CFLAGS = -DKEEP\nall:\n\tcc -DVERSION=\\\"1.0\\\" main.c\n");
        assert!(both.uncertain);
        assert_eq!(both.c, vec!["-DKEEP".to_string()]);
        let included = read_flags("include defs.mk\nCFLAGS = -DKEEP\n");
        assert!(included.uncertain);
        assert_eq!(included.c, vec!["-DKEEP".to_string()]);
        let plain = read_flags("CFLAGS = -DKEEP\nall:\n\tcc $(CFLAGS) -o demo main.c\n");
        assert!(!plain.uncertain, "{:?}", plain.c);
        assert_eq!(plain.c, vec!["-DKEEP".to_string()]);
        let defines =
            read_flags("DEFINES = -DVERSION=\\\"1.0\\\"\nall:\n\tcc $(DEFINES) -o demo main.c\n");
        assert!(defines.uncertain);
        assert!(defines.c.is_empty(), "{:?}", defines.c);
        let recurse = read_flags("all: ; $(MAKE) -C src\n");
        assert!(recurse.uncertain);
    }

    #[test]
    fn a_define_in_the_recipe_stays_advisory() {
        if !which("cc") || !which("make") {
            return;
        }
        let root = std::env::temp_dir().join(format!("sc-cc-recipe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("Makefile"),
            "all:\n\tcc -DVERSION=\\\"1.0\\\" -o demo main.c\n",
        )
        .unwrap();
        std::fs::write(
            root.join("main.c"),
            "#include <stdio.h>\nint main(void) { puts(VERSION); return 0; }\n",
        )
        .unwrap();
        let built = std::process::Command::new("make")
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(built.success());
        let steps = cpp_plan(&root);
        let types = steps.iter().find(|step| step.gate == "types").unwrap();
        let command = types.command.clone().unwrap_or_default();
        assert!(!command.contains("-DVERSION"), "{command}");
        assert!(types.advisory_failure);
        let report = run(
            PackId::Cpp,
            &root,
            Instant::now() + std::time::Duration::from_secs(30),
            None,
            true,
        );
        let gate = report.gates.iter().find(|gate| gate.id == "types").unwrap();
        assert!(!gate.pass, "{gate:?} {:?}", report.findings);
        assert!(!gate.enforced, "{gate:?} {:?}", report.findings);
        let reason = gate.reason.clone().unwrap_or_default();
        assert!(reason.contains("does not see every flag"), "{reason}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_target_specific_define_stays_advisory() {
        if !which("cc") || !which("make") {
            return;
        }
        let root = std::env::temp_dir().join(format!("sc-cc-target-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("Makefile"),
            "demo: CFLAGS += -DVERSION=\\\"1.0\\\"\nall: demo\ndemo: main.c\n\tcc $(CFLAGS) -o demo main.c\n",
        )
        .unwrap();
        std::fs::write(
            root.join("main.c"),
            "#include <stdio.h>\nint main(void) { puts(VERSION); return 0; }\n",
        )
        .unwrap();
        let built = std::process::Command::new("make")
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(built.success());
        let steps = cpp_plan(&root);
        let types = steps.iter().find(|step| step.gate == "types").unwrap();
        let command = types.command.clone().unwrap_or_default();
        assert!(!command.contains("-DVERSION"), "{command}");
        assert!(types.advisory_failure);
        let report = run(
            PackId::Cpp,
            &root,
            Instant::now() + std::time::Duration::from_secs(30),
            None,
            true,
        );
        let gate = report.gates.iter().find(|gate| gate.id == "types").unwrap();
        assert!(!gate.pass, "{gate:?} {:?}", report.findings);
        assert!(!gate.enforced, "{gate:?} {:?}", report.findings);
        let reason = gate.reason.clone().unwrap_or_default();
        assert!(reason.contains("does not see every flag"), "{reason}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_included_makefile_is_not_remade() {
        if !which("make") {
            return;
        }
        let root = std::env::temp_dir().join(format!("sc-make-include-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("Makefile"),
            "-include deps.mk\ndeps.mk:\n\techo \"# generated\" > deps.mk; touch SIDE_EFFECT\nall:\n\tcc -o demo main.c\n",
        )
        .unwrap();
        let flags = MakefileFlags::read(&root);
        assert!(flags.uncertain);
        assert!(!root.join("SIDE_EFFECT").exists());
        assert!(!root.join("deps.mk").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_shell_function_in_the_makefile_is_not_run() {
        if !which("make") {
            return;
        }
        let root = std::env::temp_dir().join(format!("sc-make-shell-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("Makefile"),
            "CFLAGS = $(shell touch SIDE_EFFECT && echo -DKEEP)\nall:\n\t@true\n",
        )
        .unwrap();
        let flags = MakefileFlags::read(&root);
        assert!(flags.uncertain);
        assert!(!root.join("SIDE_EFFECT").exists());
        // The unexpanded text is not the value `echo` would have printed.
        assert_ne!(flags.c, vec!["-DKEEP".to_string()]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_shell_assignment_in_the_makefile_is_not_run() {
        if !which("make") {
            return;
        }
        let root = std::env::temp_dir().join(format!("sc-make-bang-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("Makefile"),
            "CFLAGS != touch SIDE_EFFECT && echo -DKEEP\nall:\n\t@true\n",
        )
        .unwrap();
        let flags = MakefileFlags::read(&root);
        assert!(flags.uncertain);
        assert!(
            !flags.c.iter().any(|flag| flag.contains("KEEP")),
            "{:?}",
            flags.c
        );
        assert!(!root.join("SIDE_EFFECT").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_makefile_self_rule_is_not_run() {
        if !which("make") {
            return;
        }
        let root = std::env::temp_dir().join(format!("sc-make-self-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("Makefile"),
            "Makefile:\n\ttouch SIDE_EFFECT\nCFLAGS = -DKEEP\nall:\n\t@true\n",
        )
        .unwrap();
        let flags = MakefileFlags::read(&root);
        assert!(!flags.uncertain);
        assert_eq!(flags.c, vec!["-DKEEP".to_string()]);
        assert!(!root.join("SIDE_EFFECT").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_define_variable_in_the_recipe_stays_advisory() {
        if !which("cc") || !which("make") {
            return;
        }
        let root = std::env::temp_dir().join(format!("sc-cc-defines-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("Makefile"),
            "DEFINES = -DVERSION=\\\"1.0\\\"\nall:\n\tcc $(DEFINES) -o demo main.c\n",
        )
        .unwrap();
        std::fs::write(
            root.join("main.c"),
            "#include <stdio.h>\nint main(void) { puts(VERSION); return 0; }\n",
        )
        .unwrap();
        let built = std::process::Command::new("make")
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(built.success());
        let steps = cpp_plan(&root);
        let types = steps.iter().find(|step| step.gate == "types").unwrap();
        let command = types.command.clone().unwrap_or_default();
        assert!(!command.contains("-DVERSION"), "{command}");
        assert!(types.advisory_failure);
        let report = run(
            PackId::Cpp,
            &root,
            Instant::now() + std::time::Duration::from_secs(30),
            None,
            true,
        );
        let gate = report.gates.iter().find(|gate| gate.id == "types").unwrap();
        assert!(!gate.pass, "{gate:?} {:?}", report.findings);
        assert!(!gate.enforced, "{gate:?} {:?}", report.findings);
        let reason = gate.reason.clone().unwrap_or_default();
        assert!(reason.contains("does not see every flag"), "{reason}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_recursive_makefile_stays_advisory() {
        if !which("cc") || !which("make") {
            return;
        }
        let root = std::env::temp_dir().join(format!("sc-cc-recurse-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("Makefile"), "all: ; $(MAKE) -C src\n").unwrap();
        std::fs::write(
            root.join("src/Makefile"),
            "CFLAGS = -DVERSION=\\\"1.0\\\"\nall:\n\tcc $(CFLAGS) -o demo main.c\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/main.c"),
            "#include <stdio.h>\nint main(void) { puts(VERSION); return 0; }\n",
        )
        .unwrap();
        let built = std::process::Command::new("make")
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(built.success());
        let steps = cpp_plan(&root);
        let types = steps.iter().find(|step| step.gate == "types").unwrap();
        let command = types.command.clone().unwrap_or_default();
        assert!(!command.contains("-DVERSION"), "{command}");
        assert!(types.advisory_failure);
        let report = run(
            PackId::Cpp,
            &root,
            Instant::now() + std::time::Duration::from_secs(30),
            None,
            true,
        );
        let gate = report.gates.iter().find(|gate| gate.id == "types").unwrap();
        assert!(!gate.pass, "{gate:?} {:?}", report.findings);
        assert!(!gate.enforced, "{gate:?} {:?}", report.findings);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_eval_include_is_not_remade() {
        if !which("make") {
            return;
        }
        let root = std::env::temp_dir().join(format!("sc-make-eval-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("Makefile"),
            "$(eval -include deps.mk)\ndeps.mk:\n\techo \"# generated\" > deps.mk; touch SIDE_EFFECT\nall:\n\tcc -o demo main.c\n",
        )
        .unwrap();
        let flags = MakefileFlags::read(&root);
        assert!(flags.uncertain);
        assert!(!root.join("SIDE_EFFECT").exists());
        assert!(!root.join("deps.mk").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_file_function_is_not_run() {
        if !which("make") {
            return;
        }
        let root = std::env::temp_dir().join(format!("sc-make-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("Makefile"),
            "$(file >SIDE_EFFECT,hello)\nCFLAGS = -DKEEP\n",
        )
        .unwrap();
        let flags = MakefileFlags::read(&root);
        assert!(flags.uncertain);
        assert!(!root.join("SIDE_EFFECT").exists());
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
                advisory_failure: false,
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
                advisory_failure: false,
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
                advisory_failure: false,
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
                advisory_failure: false,
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

    #[test]
    fn node_coverage_wraps_c8_and_nyc_with_all() {
        let c8 = node_coverage_command("c8", "npm test --silent");
        assert!(c8.contains("c8 --all "), "{c8}");
        assert!(c8.contains("--reporter=json"), "{c8}");
        let nyc = node_coverage_command("nyc", "npm test --silent");
        assert!(nyc.contains("nyc --all "), "{nyc}");
        assert!(nyc.contains("--reporter=json"), "{nyc}");
        assert!(nyc.contains("--report-dir=.sc/coverage"), "{nyc}");
    }

    #[test]
    fn node_plan_without_coverage_runs_plain_npm_test() {
        let root = std::env::temp_dir().join(format!("sc-node-nocov-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.js"), "module.exports = {};\n").unwrap();
        std::fs::write(
            root.join("package.json"),
            "{\"scripts\":{\"test\":\"mocha test/*.js\"}}",
        )
        .unwrap();
        let steps = plan(PackId::Node, &root, false);
        let test = steps.iter().find(|step| step.gate == "tests").unwrap();
        let command = test.command.as_deref().unwrap_or("");
        assert!(!command.contains("c8"), "{command}");
        assert!(!command.contains("nyc"), "{command}");
        let steps = plan(PackId::Node, &root, true);
        let test = steps.iter().find(|step| step.gate == "tests").unwrap();
        let command = test.command.as_deref().unwrap_or("");
        assert!(
            command.contains("c8") || command.contains("nyc") || command.contains("npm test"),
            "{command}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

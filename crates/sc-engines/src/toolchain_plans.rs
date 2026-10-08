// SPDX-License-Identifier: MPL-2.0
//! Plan construction for the non-Rust, non-Python packs.

use std::path::Path;

use crate::pack::PackId;

use super::{docker_wrap, go_cover_path, image_present, shell_quote, which, MakefileFlags, Step};

fn via(bin: &str, command: String) -> Option<String> {
    if which(bin) {
        Some(command)
    } else if image_present() {
        Some(docker_wrap(&command))
    } else {
        None
    }
}

pub(super) fn plan_with_generated(
    pack: PackId,
    root: &Path,
    coverage: bool,
    exclude: &[String],
    include_generated: &[String],
) -> Vec<Step> {
    match pack {
        PackId::Node => node_plan(root, coverage, exclude, include_generated),
        PackId::Bash => bash_plan(root, exclude, include_generated),
        PackId::Go => go_plan(root),
        PackId::Java => java_plan_with_generated(root, exclude, include_generated),
        PackId::CSharp => csharp_plan(),
        PackId::Php => php_plan(root, exclude, include_generated),
        PackId::Cpp => cpp_plan_with_generated(root, exclude, include_generated),
        PackId::Command | PackId::Rust | PackId::Python | PackId::Web => Vec::new(),
    }
}

pub(super) fn node_plan(
    root: &Path,
    coverage: bool,
    exclude: &[String],
    include_generated: &[String],
) -> Vec<Step> {
    let files = list_files(
        root,
        &["js", "jsx", "mjs", "cjs", "ts", "tsx"],
        exclude,
        include_generated,
    );
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
        if !coverage {
            return via("npm", command);
        }
        via("c8", node_coverage_command("c8", &command))
            .or_else(|| via("nyc", node_coverage_command("nyc", &command)))
            .or_else(|| via("npm", command))
    });
    vec![
        Step {
            gate: "types",
            engine: "compile",
            command: compile,
            absent: "node is not installed".into(),
            enforce: true,
            advisory_failure: false,
        },
        Step {
            gate: "tests",
            engine: "tests",
            command: test,
            absent: "package.json has no test script".into(),
            enforce: true,
            advisory_failure: false,
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
            advisory_failure: false,
        },
    ]
}

pub(super) fn bash_plan(
    root: &Path,
    exclude: &[String],
    include_generated: &[String],
) -> Vec<Step> {
    let files = list_files(root, &["sh", "bash"], exclude, include_generated);
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
            advisory_failure: false,
        },
        Step {
            gate: "tests",
            engine: "tests",
            command: bash_tests(root, exclude, include_generated),
            absent: "bats is not installed or no .bats suite exists".into(),
            enforce: true,
            advisory_failure: false,
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
            advisory_failure: false,
        },
    ]
}

pub(super) fn go_plan(root: &Path) -> Vec<Step> {
    let _ = std::fs::remove_file(go_cover_path(root));
    vec![
        Step {
            gate: "types",
            engine: "compile",
            command: via("go", "go build ./...".into()),
            absent: "go is not installed".into(),
            enforce: true,
            advisory_failure: false,
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
            advisory_failure: false,
        },
        Step {
            gate: "lint",
            engine: "lint",
            command: via("go", "go vet ./...".into()),
            absent: "go is not installed".into(),
            enforce: true,
            advisory_failure: false,
        },
    ]
}

pub(super) fn java_plan_with_generated(
    root: &Path,
    exclude: &[String],
    include_generated: &[String],
) -> Vec<Step> {
    let files = list_files(root, &["java"], exclude, include_generated);
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
            advisory_failure: false,
        },
        Step {
            gate: "tests",
            engine: "tests",
            command: test,
            absent: test_absent.into(),
            enforce: true,
            advisory_failure: false,
        },
        Step {
            gate: "lint",
            engine: "lint",
            command: None,
            absent: "no Java linter is configured".into(),
            enforce: true,
            advisory_failure: false,
        },
    ]
}

#[cfg(test)]
pub(super) fn java_plan(root: &Path) -> Vec<Step> {
    java_plan_with_generated(root, &[], &[])
}

pub(super) fn csharp_plan() -> Vec<Step> {
    let prefix = "mkdir -p .sc/coverage .sc/nuget .sc/dotnet && DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1 DOTNET_SKIP_FIRST_TIME_EXPERIENCE=1 DOTNET_CLI_HOME=\"$PWD/.sc/dotnet\" NUGET_PACKAGES=\"$PWD/.sc/nuget\"";
    vec![
        Step {
            gate: "types",
            engine: "compile",
            command: via("dotnet", format!("{prefix} dotnet build --nologo -v q")),
            absent: "dotnet is not installed".into(),
            enforce: true,
            advisory_failure: false,
        },
        Step {
            gate: "tests",
            engine: "tests",
            command: via("dotnet", format!("{prefix} {CSHARP_TEST}")),
            absent: "dotnet is not installed".into(),
            enforce: true,
            advisory_failure: false,
        },
        Step {
            gate: "lint",
            engine: "lint",
            command: None,
            absent: "C# analyzers run as part of dotnet build".into(),
            enforce: true,
            advisory_failure: false,
        },
    ]
}

pub(super) fn php_plan(root: &Path, exclude: &[String], include_generated: &[String]) -> Vec<Step> {
    let files = list_files(root, &["php"], exclude, include_generated);
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
            advisory_failure: false,
        },
        Step {
            gate: "tests",
            engine: "tests",
            command: if root.join("vendor/bin/phpunit").is_file() {
                via("php", php_test("vendor/bin/phpunit"))
            } else if has_php_tests(root, exclude, include_generated) {
                via("phpunit", php_test("phpunit"))
            } else {
                None
            },
            absent: "phpunit is not installed".into(),
            enforce: true,
            advisory_failure: false,
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
            advisory_failure: false,
        },
    ]
}

pub(super) fn cpp_plan_with_generated(
    root: &Path,
    exclude: &[String],
    include_generated: &[String],
) -> Vec<Step> {
    let files = list_files(root, &["c", "cc", "cpp", "cxx"], exclude, include_generated);
    let (c_files, cxx_files) = split_c_family(&files);
    let flags = if c_files.is_empty() && cxx_files.is_empty() {
        MakefileFlags::none()
    } else {
        MakefileFlags::read_with_generated(root, exclude, include_generated)
    };
    let build_db = types_enforced(root);
    vec![
        Step {
            gate: "types",
            engine: "compile",
            command: cpp_compile(root, &c_files, &cxx_files, &flags),
            absent: cpp_absent(&c_files, &cxx_files),
            enforce: build_db,
            advisory_failure: flags.uncertain && !build_db,
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
            advisory_failure: false,
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
            advisory_failure: false,
        },
    ]
}

#[cfg(test)]
pub(super) fn cpp_plan(root: &Path) -> Vec<Step> {
    cpp_plan_with_generated(root, &[], &[])
}

fn bash_tests(root: &Path, exclude: &[String], include_generated: &[String]) -> Option<String> {
    let bats = list_files(root, &["bats"], exclude, include_generated);
    if bats.is_empty() {
        return None;
    }
    let libs: Vec<String> = list_files(root, &["sh"], exclude, include_generated)
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

pub(super) fn cpp_types_absent(compiler_present: bool, sources_empty: bool) -> &'static str {
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

/// True when every compiler error is a missing header or file.
/// An undeclared identifier or a syntax error returns false.
pub(super) fn include_only_failure(text: &str) -> bool {
    let mut saw_error = false;
    for line in text.lines() {
        let lower = line.to_ascii_lowercase();
        if !lower.contains("error:") {
            continue;
        }
        saw_error = true;
        let missing =
            lower.contains("file not found") || lower.contains("no such file or directory");
        if !missing {
            return false;
        }
    }
    saw_error
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

pub(super) fn missing_compiler_message(cc_missing: bool, gxx_missing: bool) -> &'static str {
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
        via(bin, check_each(prefix, files))
    }
}

fn cpp_compile(
    root: &Path,
    c_files: &[String],
    cxx_files: &[String],
    flags: &MakefileFlags,
) -> Option<String> {
    if !c_files.is_empty() || !cxx_files.is_empty() {
        let c = syntax_group(
            "cc",
            &flag_prefix("cc -fsyntax-only -x c", &flags.c),
            c_files,
        );
        let cxx = syntax_group(
            "g++",
            &flag_prefix("g++ -fsyntax-only", &flags.cxx),
            cxx_files,
        );
        if let (Some(c), Some(cxx)) = (c, cxx) {
            return Some(match (c.is_empty(), cxx.is_empty()) {
                (false, true) => c,
                (true, false) => cxx,
                (false, false) => {
                    format!("{c}; _sc_c=$?; {cxx}; _sc_x=$?; [ \"$_sc_c\" -eq 0 ] && [ \"$_sc_x\" -eq 0 ]")
                }
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

fn has_php_tests(root: &Path, exclude: &[String], include_generated: &[String]) -> bool {
    root.join("phpunit.xml").is_file()
        || root.join("phpunit.xml.dist").is_file()
        || list_files(root, &["php"], exclude, include_generated)
            .iter()
            .any(|file| file.contains("Test") || file.contains("/tests/"))
}

/// Wrap an npm test command so unloaded files are still measured at 0%.
/// c8 and nyc both need `--all`; without it a never-required module disappears
/// from the report and CRAP cannot fail on it (#120 / #79).
pub(super) fn node_coverage_command(tool: &str, command: &str) -> String {
    match tool {
        "c8" => format!(
            "mkdir -p .sc/coverage && c8 --all --reporter=json --reports-dir=.sc/coverage {command}"
        ),
        "nyc" => format!(
            "mkdir -p .sc/coverage && nyc --all --reporter=json --report-dir=.sc/coverage {command}"
        ),
        other => panic!("unsupported node coverage tool: {other}"),
    }
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

pub(super) fn eslint_config(root: &Path) -> bool {
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

fn list_files(
    root: &Path,
    exts: &[&str],
    exclude: &[String],
    include_generated: &[String],
) -> Vec<String> {
    let mut out: Vec<String> =
        sc_graph::walk_files(root, root, exclude, include_generated, Some(5))
            .into_iter()
            .filter(|path| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| exts.contains(&ext))
            })
            .filter_map(|path| {
                path.strip_prefix(root)
                    .ok()
                    .map(|rel| rel.to_string_lossy().replace('\\', "/"))
            })
            .collect();
    out.sort();
    out.truncate(200);
    out
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
pub(super) fn tsc_and_node(has_tsconfig: bool, ts: &[String], js: &[String]) -> String {
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

fn flag_prefix(base: &str, flags: &[String]) -> String {
    let mut prefix = base.to_string();
    for flag in flags {
        prefix.push(' ');
        prefix.push_str(&shell_quote(flag));
    }
    prefix
}

/// Run the check on every file. A missing header in an earlier file must not
/// hide a later syntax error.
fn check_each(prefix: &str, files: &[String]) -> String {
    let checks = files
        .iter()
        .map(|file| format!("{prefix} {} || status=1", shell_quote(file)))
        .collect::<Vec<_>>()
        .join("; ");
    format!("( status=0; {checks}; exit $status )")
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

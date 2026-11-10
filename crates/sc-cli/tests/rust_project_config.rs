// SPDX-License-Identifier: MIT
use std::path::{Path, PathBuf};
use std::process::Command;

fn run_sc(root: &Path, args: &[&str]) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_sc"))
        .current_dir(root)
        .args(args)
        .env("SCORECARD_NO_AGENT_SKILLS", "1")
        .output()
        .expect("spawn sc");
    (
        output.status.code().unwrap_or(101),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn temp_project(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "sc-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    root
}

fn write_analyzer(root: &Path, text: &str) {
    std::fs::write(root.join("analyzer.toml"), text).unwrap();
}

#[test]
fn invalid_budget_and_test_args_exit_with_the_fix() {
    let root = temp_project("bad-budget");
    write_analyzer(&root, "budget_seconds = 0\n");
    let (code, stdout, stderr) = run_sc(&root, &["analyze", ".", "--format", "json", "--quiet"]);
    assert_eq!(code, 2, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.contains("budget_seconds = 120"), "{stderr}");
    assert!(stdout.contains("config.invalid"), "{stdout}");
    let _ = std::fs::remove_dir_all(&root);

    let root = temp_project("bad-args");
    write_analyzer(
        &root,
        "[rust]\ntest_args = [\"--manifest-path\", \"other/Cargo.toml\"]\n",
    );
    let (code, stdout, stderr) = run_sc(&root, &["analyze", ".", "--format", "json", "--quiet"]);
    assert_eq!(code, 2, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.contains("Remove --manifest-path"), "{stderr}");
    assert!(stdout.contains("config.invalid"), "{stdout}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn rust_test_args_reach_cargo_test() {
    let root = temp_project("test-args");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"test_args\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[features]\nextra = []\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/lib.rs"),
        r#"pub fn ready() -> i32 { 1 }

#[cfg(feature = "extra")]
#[test]
fn extra_feature_is_enabled() {
    assert_eq!(ready(), 1);
}

#[cfg(not(feature = "extra"))]
#[test]
fn extra_feature_is_enabled() {
    panic!("cargo test ran without --features extra");
}

#[test]
fn ignored_case() {
    panic!("harness did not receive --skip ignored_case");
}
"#,
    )
    .unwrap();
    write_analyzer(
        &root,
        r#"budget_seconds = 180
[gates]
fail_on = ["tests"]
[engines]
coverage = false
[commands]
lint = ""
[rust]
test_args = ["--features", "extra", "--", "--test-threads=1", "--skip", "ignored_case"]
"#,
    );
    let (code, stdout, stderr) = run_sc(&root, &["analyze", ".", "--format", "json", "--quiet"]);
    let card: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("stdout was not JSON ({err})\n{stdout}\n{stderr}"));
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    let tests = card["gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["id"] == "tests")
        .unwrap();
    assert_eq!(tests["pass"], true, "{card}");
    let command = card["runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|run| run["engine"] == "tests")
        .unwrap();
    let line = command["command"].as_str().unwrap();
    assert!(line.contains("--features extra"), "{line}");
    assert!(line.contains("--test-threads=1"), "{line}");
    assert!(line.contains("--skip ignored_case"), "{line}");
    assert_eq!(command["exit_code"], 0, "{command}");
    let _ = std::fs::remove_dir_all(&root);
}

fn write_slow_crate(root: &Path, budget: &str) {
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"slow_budget\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/lib.rs"),
        "#[test]\nfn holds_the_clock() {\n    std::thread::sleep(std::time::Duration::from_secs(45));\n}\n",
    )
    .unwrap();
    write_analyzer(
        root,
        &format!(
            "budget_seconds = {budget}\n[gates]\nfail_on = [\"tests\"]\n[engines]\ncoverage = false\n[commands]\nlint = \"\"\n"
        ),
    );
}

fn assert_timed_out(code: i32, stdout: &str, stderr: &str) {
    let card: serde_json::Value = serde_json::from_str(stdout)
        .unwrap_or_else(|err| panic!("stdout was not JSON ({err})\n{stdout}\n{stderr}"));
    assert_eq!(code, 2, "stdout:\n{stdout}\nstderr:\n{stderr}");
    let timed_out = card["findings"].as_array().unwrap().iter().any(|finding| {
        finding["message"]
            .as_str()
            .is_some_and(|message| message.contains("timed out"))
    });
    assert!(timed_out, "stdout:\n{stdout}\nstderr:\n{stderr}");
}

#[test]
fn project_budget_is_used_and_the_flag_overrides_it() {
    let configured = temp_project("budget-file");
    write_slow_crate(&configured, "8");
    let (code, stdout, stderr) = run_sc(
        &configured,
        &["analyze", ".", "--format", "json", "--quiet"],
    );
    assert_timed_out(code, &stdout, &stderr);
    let _ = std::fs::remove_dir_all(&configured);

    let flagged = temp_project("budget-flag");
    write_slow_crate(&flagged, "600");
    let (code, stdout, stderr) = run_sc(
        &flagged,
        &[
            "analyze",
            ".",
            "--format",
            "json",
            "--quiet",
            "--budget-seconds",
            "8",
        ],
    );
    assert_timed_out(code, &stdout, &stderr);
    let _ = std::fs::remove_dir_all(&flagged);
}

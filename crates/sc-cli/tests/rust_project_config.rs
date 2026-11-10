// SPDX-License-Identifier: MIT
use std::fs;
use std::path::Path;
use std::process::Command;

fn run(root: &Path, args: &[&str]) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_sc"))
        .arg("analyze")
        .arg(root)
        .args(args)
        .env("SCORECARD_NO_AGENT_SKILLS", "1")
        .output()
        .unwrap();
    (
        output.status.code().unwrap_or(127),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn scratch(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "sc-rust-cfg-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    root
}

#[test]
fn rejects_invalid_budget_and_test_args_before_analysis() {
    let root = scratch("invalid");
    fs::write(root.join("analyzer.toml"), "budget_seconds = 0\n").unwrap();
    let (code, stdout, stderr) = run(&root, &["--format", "json", "--quiet"]);
    assert_eq!(code, 2, "{stdout}{stderr}");
    assert!(
        stderr.contains("budget_seconds must be at least 1"),
        "{stderr}"
    );
    assert!(stderr.contains("budget_seconds = 120"), "{stderr}");

    fs::write(
        root.join("analyzer.toml"),
        "[commands]\ntest_args = [\"--workspace\", \"--locked\"]\n",
    )
    .unwrap();
    let (code, stdout, stderr) = run(&root, &["--format", "json", "--quiet"]);
    assert_eq!(code, 2, "{stdout}{stderr}");
    assert!(stderr.contains("cannot include `--workspace`"), "{stderr}");
    assert!(
        stdout.contains("Remove `--workspace` from test_args"),
        "{stdout}"
    );
    let card: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(
        card["runs"].as_array().unwrap().is_empty(),
        "invalid test args must not start pack commands: {stdout}"
    );

    fs::write(
        root.join("analyzer.toml"),
        "[commands]\ntest_args = \"--locked -- --test-threads=1\"\n",
    )
    .unwrap();
    let (code, stdout, stderr) = run(&root, &["--format", "json", "--quiet"]);
    assert_eq!(code, 2, "{stdout}{stderr}");
    assert!(stderr.contains("test_args"), "{stderr}");

    let flag = Command::new(env!("CARGO_BIN_EXE_sc"))
        .args(["analyze", ".", "--budget-seconds", "0"])
        .output()
        .unwrap();
    assert_eq!(flag.status.code(), Some(2));
    let flag_err = String::from_utf8_lossy(&flag.stderr);
    assert!(flag_err.contains("--budget-seconds"), "{flag_err}");
    assert!(flag_err.contains('0'), "{flag_err}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn configured_test_args_select_the_feature_and_the_filter() {
    let root = scratch("features");
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"feature_gate\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[features]\nextra = []\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        r#"pub fn value() -> u8 { 1 }

#[test]
fn do_not_run() { panic!("filter missed"); }

#[cfg(feature = "extra")]
#[test]
fn extra_on() { assert_eq!(value(), 1); }

#[cfg(not(feature = "extra"))]
#[test]
fn extra_on() { panic!("feature extra was not enabled"); }
"#,
    )
    .unwrap();
    fs::write(
        root.join("analyzer.toml"),
        "budget_seconds = 180\n[engines]\ncoverage = false\n[commands]\nlint = \"\"\ntest_args = [\"--features\", \"extra\", \"extra_on\", \"--\", \"--test-threads=1\"]\n",
    )
    .unwrap();
    let (code, stdout, stderr) = run(
        &root,
        &["--format", "json", "--quiet", "--fail-on", "types,tests"],
    );
    assert_eq!(code, 0, "{stdout}{stderr}");
    let card: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let tests: Vec<&str> = card["runs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|run| run["engine"] == "tests")
        .map(|run| run["command"].as_str().unwrap())
        .collect();
    assert_eq!(
        tests,
        ["cargo test --workspace --features extra extra_on -- --test-threads=1"]
    );
    assert!(
        !stdout.contains("test.failed"),
        "feature or filter was not passed to cargo test: {stdout}"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn config_budget_is_used_until_the_flag_overrides_it() {
    let root = scratch("budget");
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"slow_gate\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "#[test]\nfn holds() {\n    std::thread::sleep(std::time::Duration::from_secs(30));\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("analyzer.toml"),
        "budget_seconds = 1\n[engines]\ncoverage = false\n[commands]\nlint = \"\"\n",
    )
    .unwrap();
    let (code, stdout, stderr) = run(
        &root,
        &["--format", "json", "--quiet", "--fail-on", "types,tests"],
    );
    assert_eq!(code, 2, "{stdout}{stderr}");
    assert!(stdout.contains("--budget-seconds 2"), "{stdout}{stderr}");
    assert!(stdout.contains("budget_seconds = 2"), "{stdout}{stderr}");

    let (code, stdout, stderr) = run(
        &root,
        &[
            "--format",
            "json",
            "--quiet",
            "--fail-on",
            "types,tests",
            "--budget-seconds",
            "4",
        ],
    );
    assert_eq!(code, 2, "{stdout}{stderr}");
    assert!(stdout.contains("--budget-seconds 8"), "{stdout}{stderr}");
    assert!(stdout.contains("budget_seconds = 8"), "{stdout}{stderr}");
    assert!(
        !stdout.contains("--budget-seconds 2"),
        "flag did not override the config budget: {stdout}"
    );
    let _ = fs::remove_dir_all(&root);
}

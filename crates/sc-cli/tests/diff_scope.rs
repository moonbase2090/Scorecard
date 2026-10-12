use std::path::Path;
use std::process::Command;

fn git_here(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "sc")
        .env("GIT_AUTHOR_EMAIL", "sc@example.com")
        .env("GIT_COMMITTER_NAME", "sc")
        .env("GIT_COMMITTER_EMAIL", "sc@example.com")
        .status()
        .expect("spawn git");
    assert!(status.success(), "{args:?}");
}

fn single_commit_on(label: &str, branch: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("sc-diff-scope-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"diff_scope\"\nversion = \"0.1.0\"\nedition = \"2021\"\npublish = false\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/lib.rs"), "pub fn value() -> i32 { 1 }\n").unwrap();
    git_here(&dir, &["init"]);
    git_here(&dir, &["add", "Cargo.toml", "src/lib.rs"]);
    git_here(&dir, &["commit", "-m", "single"]);
    git_here(&dir, &["branch", "-M", branch]);
    dir
}

fn analyze(dir: &Path, args: &[&str]) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_sc"))
        .current_dir(dir)
        .arg("analyze")
        .args(args)
        .output()
        .expect("spawn sc");
    (
        output.status.code().unwrap_or(101),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

fn stderr_lines(stderr: &str) -> Vec<&str> {
    stderr.lines().filter(|line| !line.is_empty()).collect()
}

#[test]
fn bare_diff_on_a_single_commit_default_branch_does_not_pass() {
    for branch in ["main", "master"] {
        let dir = single_commit_on(&format!("bare-{branch}"), branch);
        let (code, stdout, stderr) = analyze(
            &dir,
            &[".", "--diff", "--format", "json", "--budget-seconds", "180"],
        );
        assert_eq!(
            code, 2,
            "{branch}: bare --diff must not exit 0\nstdout={stdout}\nstderr={stderr}"
        );
        let card: serde_json::Value = serde_json::from_str(&stdout).expect("json");
        assert_ne!(
            card["verdict"], "pass",
            "{branch}: verdict must not pass\n{card}"
        );
        let lines = stderr_lines(&stderr);
        assert_eq!(lines.len(), 1, "{branch}: stderr={stderr:?}");
        assert!(
            lines[0].contains("fetch-depth: 0"),
            "{branch}: stderr must name the fix: {stderr}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn missing_explicit_base_is_not_a_pass_in_any_format() {
    let dir = single_commit_on("missing", "main");
    for format in ["json", "pretty", "md", "sarif", "html"] {
        let (code, stdout, stderr) = analyze(
            &dir,
            &[
                ".",
                "--diff",
                "does-not-exist",
                "--format",
                format,
                "--budget-seconds",
                "180",
            ],
        );
        assert_eq!(
            code, 2,
            "{format}: missing base must exit 2\nstdout={stdout}\nstderr={stderr}"
        );
        let lines = stderr_lines(&stderr);
        assert_eq!(lines.len(), 1, "{format}: stderr={stderr:?}");
        assert!(
            lines[0].contains("--diff does-not-exist:"),
            "{format}: stderr must name the ref: {stderr}"
        );
        assert!(
            lines[0].contains("fetch-depth: 0"),
            "{format}: stderr must name the fix: {stderr}"
        );
        if format == "json" {
            let card: serde_json::Value = serde_json::from_str(&stdout).expect("json");
            assert_ne!(card["verdict"], "pass", "{card}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
